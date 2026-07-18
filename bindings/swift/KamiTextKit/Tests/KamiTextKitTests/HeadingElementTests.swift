import Testing
@testable import KamiTextKit

/// Headings surface through `KamiEngine.elements(in:)` as `.heading` elements
/// whose `level` (the C 'checked' byte) is 1–6 and whose `auxRange` is the
/// title byte range net of markers and surrounding whitespace — the outline
/// source WP2 app code reads against. Covers ATX, setext, and confirms a
/// heading-shaped line inside a fence emits no `.heading` element.
@MainActor
struct HeadingElementTests {
    private func titleString(_ engine: KamiEngine, _ aux: Range<UInt32>) throws -> String {
        let utf8 = Array(try engine.text().utf8)
        return String(decoding: utf8[Int(aux.lowerBound)..<Int(aux.upperBound)], as: UTF8.self)
    }

    @Test func atxSetextHeadingsFencedDecoyIgnored() throws {
        let engine = try KamiEngine(text: "# One\n\nTitle\n=====\n\n## Two ##\n\n```\n# not a heading\n```\n")
        let headings = try engine.elements(in: 0..<engine.lenBytes)
            .filter { $0.kind == .heading }
        #expect(headings.count == 3)
        #expect(headings.map(\.level) == [1, 1, 2])
        #expect(try headings.map { try titleString(engine, $0.auxRange) } == ["One", "Title", "Two"])
    }

    @Test func emptyHeadingHasZeroWidthTitle() throws {
        let engine = try KamiEngine(text: "##\n")
        let heading = try #require(
            try engine.elements(in: 0..<engine.lenBytes).first { $0.kind == .heading }
        )
        #expect(heading.level == 2)
        #expect(heading.auxRange.lowerBound == heading.auxRange.upperBound)
        #expect(try titleString(engine, heading.auxRange) == "")
    }

    /// The overloaded C `checked` byte must decode per kind: a heading is
    /// never `checked`, and a checked task never reports a heading `level`.
    @Test func checkedByteDoesNotLeakAcrossKinds() throws {
        let engine = try KamiEngine(text: "# H\n\n- [x] done\n")
        let elements = try engine.elements(in: 0..<engine.lenBytes)
        let heading = try #require(elements.first { $0.kind == .heading })
        let task = try #require(elements.first { $0.kind == .task })
        #expect(heading.level == 1)
        #expect(heading.checked == false)
        #expect(task.checked == true)
        #expect(task.level == 0)
    }
}
