import Testing
@testable import KamiTextKit

/// Wikilinks surface through `KamiEngine.elements(in:)` as `.wikilink`
/// elements whose `auxRange` is the target byte range — the range R6/R8 app
/// code resolves against the index. Covers the plain `[[target]]` and piped
/// `[[target|alias]]` forms, and confirms the target excludes the alias.
@MainActor
struct WikilinkElementTests {
    private func targetString(_ engine: KamiEngine, _ aux: Range<UInt32>) throws -> String {
        let utf8 = Array(try engine.text().utf8)
        return String(decoding: utf8[Int(aux.lowerBound)..<Int(aux.upperBound)], as: UTF8.self)
    }

    @Test func plainWikilinkTargetsWholeName() throws {
        let engine = try KamiEngine(text: "See [[Note]] here")
        let elements = try engine.elements(in: 0..<engine.lenBytes)
        #expect(elements.count == 1)
        let link = try #require(elements.first)
        #expect(link.kind == .wikilink)
        #expect(link.range == 4..<12)     // [[Note]]
        #expect(link.auxRange == 6..<10)  // Note
        #expect(try targetString(engine, link.auxRange) == "Note")
    }

    @Test func pipedWikilinkTargetsBeforePipe() throws {
        let engine = try KamiEngine(text: "See [[target|alias]] here")
        let elements = try engine.elements(in: 0..<engine.lenBytes)
        #expect(elements.count == 1)
        let link = try #require(elements.first)
        #expect(link.kind == .wikilink)
        #expect(link.range == 4..<20)     // [[target|alias]]
        #expect(link.auxRange == 6..<12)  // target
        #expect(try targetString(engine, link.auxRange) == "target")
    }

    @Test func wikilinkBodyStylesAsLinkAndBracketsConceal() throws {
        // Caret on the trailing line conceals the brackets; the visible body
        // still carries the LINK kind (no new style bit added).
        let engine = try KamiEngine(text: "[[Note]]\nx")
        _ = try engine.setSelection(9..<9)
        let segments = try engine.segments(in: 0..<engine.lenBytes)
        let body = try #require(segments.first { $0.kinds.contains(.link) })
        #expect(body.concealed == false)
        #expect(body.range == 2..<6)      // Note
        let markers = segments.filter { $0.kinds.contains(.marker) }
        #expect(markers.isEmpty == false)
        #expect(markers.allSatisfy { $0.concealed })
    }
}
