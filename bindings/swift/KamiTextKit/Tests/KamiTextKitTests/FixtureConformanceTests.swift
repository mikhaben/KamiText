import Foundation
import Testing
@testable import KamiTextKit

/// Gate 2: replays the 3 required conformance fixtures through `KamiEngine`
/// and asserts the resulting segments match `expect.segments` element-for-
/// element (byte range, UTF-16 range, kind set, concealed). Fixture files
/// live at `<repo>/fixtures/`, resolved relative to this source file so the
/// tests run from any checkout location.
@MainActor
struct FixtureConformanceTests {
    private static let fixturesDirectory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // KamiTextKitTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // KamiTextKit
        .deletingLastPathComponent() // swift
        .deletingLastPathComponent() // bindings
        .deletingLastPathComponent() // repo root
        .appendingPathComponent("fixtures")
        .path

    @Test func compositionHeadingStrong() throws {
        try runFixture(named: "composition-heading-strong")
    }

    @Test func emojiAstralMidMarker() throws {
        try runFixture(named: "emoji-astral-mid-marker")
    }

    @Test func concealAwayFromCaret() throws {
        try runFixture(named: "conceal-away-from-caret")
    }

    @Test func elementRevealInlineCaretInside() throws {
        try runFixture(named: "element-reveal-inline-caret-inside")
    }

    @Test func elementRevealInlineCaretOutside() throws {
        try runFixture(named: "element-reveal-inline-caret-outside")
    }

    @Test func elementRevealBoundaryStart() throws {
        try runFixture(named: "element-reveal-boundary-start")
    }

    @Test func elementRevealBoundaryEnd() throws {
        try runFixture(named: "element-reveal-boundary-end")
    }

    @Test func elementRevealAdjacentElements() throws {
        try runFixture(named: "element-reveal-adjacent-elements")
    }

    @Test func elementRevealBlockMarkers() throws {
        try runFixture(named: "element-reveal-block-markers")
    }

    @Test func elementRevealSelectionSpan() throws {
        try runFixture(named: "element-reveal-selection-span")
    }

    @Test func elementRevealSelectionEndpoint() throws {
        try runFixture(named: "element-reveal-selection-endpoint")
    }

    @Test func elementRevealNested() throws {
        try runFixture(named: "element-reveal-nested")
    }

    @Test func elementRevealMultilineSpan() throws {
        try runFixture(named: "element-reveal-multiline-span")
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
                _ = try engine.setSelection(start..<end)
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
