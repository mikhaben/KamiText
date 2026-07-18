import Foundation
import Testing
@testable import KamiTextKit

/// Gate 2: replays EVERY committed conformance fixture through `KamiEngine`
/// and asserts the resulting segments match `expect.segments` element-for-
/// element (byte range, UTF-16 range, kind set, concealed). Fixture files
/// live at `<repo>/fixtures/`, resolved relative to this source file so the
/// tests run from any checkout location. The list is discovered from the
/// directory — a committed fixture that isn't replayed cannot exist.
@MainActor
struct FixtureConformanceTests {
    private nonisolated static let fixturesDirectory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // KamiTextKitTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // KamiTextKit
        .deletingLastPathComponent() // swift
        .deletingLastPathComponent() // bindings
        .deletingLastPathComponent() // repo root
        .appendingPathComponent("fixtures")
        .path

    /// Every `*.json` in `fixtures/`, sorted for stable test identity.
    /// `nonisolated`: `@Test(arguments:)` evaluates off the main actor.
    private nonisolated static let allFixtureNames: [String] = {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: fixturesDirectory))?
            .filter { $0.hasSuffix(".json") }
            .map { String($0.dropLast(".json".count)) }
            .sorted() ?? []
        precondition(!names.isEmpty, "no fixtures found at \(fixturesDirectory)")
        return names
    }()

    @Test(arguments: allFixtureNames)
    func replays(fixture name: String) throws {
        try runFixture(named: name)
    }

    private func runFixture(named name: String) throws {
        let url = URL(fileURLWithPath: "\(Self.fixturesDirectory)/\(name).json")
        let data = try Data(contentsOf: url)
        let fixture = try JSONDecoder().decode(Fixture.self, from: data)

        let extensions = fixture.options.extensions.reduce(into: KamiEngine.Extensions()) { result, ext in
            switch ext {
            case "tables": result.insert(.tables)
            case "task_lists": result.insert(.taskLists)
            case "strikethrough": result.insert(.strikethrough)
            case "wikilinks": result.insert(.wikilinks)
            default: break
            }
        }
        let reveal: KamiEngine.RevealMode
        switch fixture.options.reveal {
        case "none": reveal = .none
        case "line": reveal = .line
        case "block": reveal = .block
        case "element": reveal = .element
        default:
            Issue.record("unknown reveal string \"\(fixture.options.reveal)\" in fixture \(name)")
            return
        }

        let engine = try KamiEngine(text: fixture.text, options: .init(extensions: extensions, reveal: reveal))

        for op in fixture.ops {
            switch op {
            case let .edit(start, end, insert):
                _ = try engine.applyEdit(start..<end, replacement: insert)
            case let .selection(start, end):
                // The Swift API can't express a reversed selection (`Range`
                // enforces order) — hosts hand it NSRanges, never reversed.
                // The engine normalizes either way (covered by the Rust
                // suite), so replay the normalized form.
                _ = try engine.setSelection(min(start, end)..<max(start, end))
            }
        }

        let actualSegments = try engine.segments(in: 0..<engine.lenBytes)
        #expect(actualSegments.count == fixture.expect.segments.count, "segment count mismatch for \(name)")

        for (actual, expected) in zip(actualSegments, fixture.expect.segments) {
            #expect(actual.range == expected.range.start..<expected.range.end, "byte range mismatch for \(name)")
            #expect(
                actual.utf16Range == expected.range.utf16Start..<expected.range.utf16End,
                "utf16 range mismatch for \(name)"
            )
            #expect(actual.kinds == KamiKindSet(fixtureNames: expected.kinds), "kinds mismatch for \(name)")
            #expect(actual.concealed == expected.concealed, "concealed mismatch for \(name)")
        }

        #expect(try engine.text() == fixture.expect.text, "text mismatch for \(name)")
        #expect(engine.lenBytes == fixture.expect.lenBytes, "lenBytes mismatch for \(name)")
        #expect(engine.lenUtf16 == fixture.expect.lenUtf16, "lenUtf16 mismatch for \(name)")
    }
}
