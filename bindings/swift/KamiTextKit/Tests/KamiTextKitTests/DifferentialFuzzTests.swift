import Foundation
import Testing
@testable import KamiTextKit
#if os(macOS)
import AppKit

/// Differential fuzz-through-the-view harness. Drives a
/// real `FuzzHost` (TextKit 2 `NSTextView` + `KamiTextSync`) through random
/// edit/selection/undo ops and, after each one, re-derives ground truth from
/// a **fresh** `KamiEngine` over the live storage's current string —
/// `setSelection` replayed to the live view's selection, `applyFull`ed into
/// a detached `NSTextStorage` — then compares it against the live storage
/// the incremental patches produced. A mismatch means the incremental path
/// (patch application, selection-aware reveal, or the sync's desync
/// recovery) diverged from a full reparse — the harness's whole reason to
/// exist.
///
/// `.serialized`: runs share `NSApplication.shared`/window first-responder
/// state (same reason as `MarkedTextDesyncTests`).
@Suite("Differential fuzz through the view", .serialized)
@MainActor
struct DifferentialFuzzTests {
    private static let defaultSeed: UInt64 = 1
    /// Edmund's own seed set — kept for cross-reference, not shared code.
    private static let fullSeeds: [UInt64] = [1, 7, 1234, 0xBEEF]

    /// Budget: 1 seed × 40 iterations (plus 20 on the corpus stress doc),
    /// both reveal modes — keeps full `swift test` under the ~60s ceiling
    /// (M4 plan item 5).
    @Test func defaultBudgetMatchesOracle() throws {
        for mode: KamiEngine.RevealMode in [.line, .element] {
            try runFuzz(corpusIndex: 0, seed: Self.defaultSeed, iterations: 40, mode: mode)
            try runFuzz(corpusIndex: 4, seed: Self.defaultSeed, iterations: 20, mode: mode)
        }
    }

    /// Budget: 4 seeds × 120 iterations plus the corpus stress doc, both
    /// reveal modes — gated behind `KAMI_FUZZ=1` so it never runs in the
    /// default `swift test` pass.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["KAMI_FUZZ"] == "1"))
    func fullBudgetMatchesOracle() throws {
        for mode: KamiEngine.RevealMode in [.line, .element] {
            for (index, seed) in Self.fullSeeds.enumerated() {
                try runFuzz(corpusIndex: index, seed: seed, iterations: 120, mode: mode)
            }
            try runFuzz(corpusIndex: 4, seed: Self.fullSeeds[0], iterations: 120, mode: mode)
        }
    }

    // MARK: - Fuzz loop

    private func runFuzz(corpusIndex: Int, seed: UInt64, iterations: Int, mode: KamiEngine.RevealMode) throws {
        let options = KamiEngine.Options(reveal: mode)
        let applier = KamiTextStorageApplier()
        let document = try corpusDocument(index: corpusIndex, seed: seed)
        let host = FuzzHost(text: document, options: options)
        var rng = SeededGenerator(seed: seed)

        guard try oracleMatches(host: host, options: options, applier: applier, context: "seed=\(seed) mode=\(mode) op=initial-seed") else {
            return
        }

        for i in 0..<iterations {
            let kind = pickOpKind(&rng)
            let description = applyOp(kind, host: host, rng: &rng)
            let context = "seed=\(seed) mode=\(mode) iteration=\(i) op=\(description)"
            guard try oracleMatches(host: host, options: options, applier: applier, context: context) else {
                return
            }
        }
    }

    // MARK: - Corpus (M4 plan item 2: generator + 2-3 fixture bodies)

    private static let fixturesDirectory: String = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // KamiTextKitTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // KamiTextKit
        .deletingLastPathComponent() // swift
        .deletingLastPathComponent() // bindings
        .deletingLastPathComponent() // repo root
        .appendingPathComponent("fixtures")
        .path

    /// Real-markdown corpus at the repo root — same `#filePath` walk as
    /// `fixturesDirectory`, NOT an SPM resource.
    private static let corporaDirectory: String = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // KamiTextKitTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // KamiTextKit
        .deletingLastPathComponent() // swift
        .deletingLastPathComponent() // bindings
        .deletingLastPathComponent() // repo root
        .appendingPathComponent("corpora")
        .path

    private func corpusDocument(index: Int, seed: UInt64) throws -> String {
        switch index {
        case 1: return try fixtureBody(named: "links-and-images")
        case 2: return try fixtureBody(named: "cjk-heading-and-body")
        case 3: return try fixtureBody(named: "table-block")
        case 4:
            // The corpus's grapheme stress doc: ZWJ families, skin tones,
            // tag-sequence flags, keycaps flush against delimiters.
            let url = URL(fileURLWithPath: "\(Self.corporaDirectory)/emoji-zwj-stress.md")
            return try String(contentsOf: url, encoding: .utf8)
        default: return makeLargeMarkdown(approximateBytes: 6000, seed: seed)
        }
    }

    private func fixtureBody(named name: String) throws -> String {
        let url = URL(fileURLWithPath: "\(Self.fixturesDirectory)/\(name).json")
        let data = try Data(contentsOf: url)
        return try JSONDecoder().decode(Fixture.self, from: data).text
    }

    // MARK: - Op generation and application

    private enum FuzzOpKind {
        case insert, delete, replace, moveSelection, undo
    }

    private func pickOpKind(_ rng: inout SeededGenerator) -> FuzzOpKind {
        switch rng.nextInt(upperBound: 100) {
        case 0..<30: return .insert
        case 30..<50: return .delete
        case 50..<70: return .replace
        case 70..<95: return .moveSelection
        default: return .undo
        }
    }

    /// Applies one op through the live view and returns a description for
    /// failure repro (seed + iteration index alone already replay the exact
    /// same op deterministically; the description just saves re-deriving it
    /// by hand).
    @discardableResult
    private func applyOp(_ kind: FuzzOpKind, host: FuzzHost, rng: inout SeededGenerator) -> String {
        let text = host.storage.string as NSString
        switch kind {
        case .insert:
            let at = clampedBoundary(rng.nextInt(upperBound: text.length + 1), in: text)
            let fragment = randomFragment(&rng)
            host.view.insertText(fragment, replacementRange: NSRange(location: at, length: 0))
            return "insert \(fragment.debugDescription) at \(at)"
        case .delete:
            let range = randomRange(&rng, in: text, maxSpan: 40)
            host.view.insertText("", replacementRange: range)
            return "delete range \(range)"
        case .replace:
            let range = randomRange(&rng, in: text, maxSpan: 40)
            let fragment = randomFragment(&rng)
            host.view.insertText(fragment, replacementRange: range)
            return "replace range \(range) with \(fragment.debugDescription)"
        case .moveSelection:
            let range: NSRange
            if rng.chance(0.5) {
                let at = clampedBoundary(rng.nextInt(upperBound: text.length + 1), in: text)
                range = NSRange(location: at, length: 0)
            } else {
                range = randomRange(&rng, in: text, maxSpan: 20)
            }
            host.view.setSelectedRange(range)
            return "moveSelection to \(range)"
        case .undo:
            host.view.undoManager?.undo()
            return "undo"
        }
    }

    /// Snaps `raw` (clamped into `0...text.length`) to the nearest composed-
    /// character-sequence boundary so generated ranges never split a
    /// surrogate pair or grapheme cluster (the document corpus includes
    /// astral emoji and CJK).
    private func clampedBoundary(_ raw: Int, in text: NSString) -> Int {
        let length = text.length
        let offset = max(0, min(raw, length))
        guard offset > 0, offset < length else { return offset }
        return text.rangeOfComposedCharacterSequence(at: offset).location
    }

    private func randomRange(_ rng: inout SeededGenerator, in text: NSString, maxSpan: Int) -> NSRange {
        let length = text.length
        let start = clampedBoundary(rng.nextInt(upperBound: length + 1), in: text)
        let spanCap = min(maxSpan, length - start)
        let rawEnd = start + rng.nextInt(upperBound: spanCap + 1)
        let end = clampedBoundary(rawEnd, in: text)
        return NSRange(location: start, length: end - start)
    }

    // MARK: - Oracle (M4 plan item 3)

    /// Re-derives ground truth from a fresh engine over the live storage's
    /// current string, replays the live selection onto it, and compares a
    /// detached `applyFull` render against the live storage. Returns
    /// `false` (after recording the failure) on the first mismatch so a
    /// real divergence doesn't cascade into a flood of downstream failures
    /// from an already-diverged state.
    private func oracleMatches(
        host: FuzzHost, options: KamiEngine.Options, applier: KamiTextStorageApplier, context: String
    ) throws -> Bool {
        let storage = host.storage
        guard let engine = host.sync.engine else {
            Issue.record("\(context): sync.engine is nil")
            return false
        }
        guard engine.lenUtf16 == UInt32(storage.length) else {
            Issue.record("\(context): invariant violated — engine.lenUtf16=\(engine.lenUtf16) storage.length=\(storage.length)")
            return false
        }

        let oracleEngine = try KamiEngine(text: storage.string, options: options)
        let live = host.view.selectedRange()
        let byteStart = try oracleEngine.utf16ToByte(UInt32(live.location))
        let byteEnd = try oracleEngine.utf16ToByte(UInt32(live.location + live.length))
        _ = try oracleEngine.setSelection(byteStart..<byteEnd)

        let oracleStorage = NSTextStorage(string: storage.string)
        try applier.applyFull(engine: oracleEngine, to: oracleStorage)

        guard storage.string == oracleStorage.string else {
            Issue.record("\(context): string mismatch — live=\(storage.string.debugDescription) oracle=\(oracleStorage.string.debugDescription)")
            return false
        }
        // Normalize both sides with an explicit attribute-fixing pass before
        // comparing: WHEN AppKit fixes attributes differs by apply path
        // (`setAttributedString` fixes eagerly, `setAttributes` lazily), and
        // fixing font-substitutes whole composed character sequences — so raw
        // pre-fix attributes diverge on any emoji/dingbat that a random edit
        // places against a styled boundary (PLATFORM_BUGS.md #4). This
        // harness tests incremental-vs-fresh engine agreement, not platform
        // fixing timing; post-fix attributes are what actually renders.
        let fixedLive = NSMutableAttributedString(attributedString: storage)
        fixedLive.fixAttributes(in: NSRange(location: 0, length: fixedLive.length))
        let fixedOracle = NSMutableAttributedString(attributedString: oracleStorage)
        fixedOracle.fixAttributes(in: NSRange(location: 0, length: fixedOracle.length))
        if let mismatch = firstAttributeMismatch(live: fixedLive, oracle: fixedOracle) {
            Issue.record("\(context): \(mismatch)")
            return false
        }
        return true
    }

    /// Walks both attributed strings run-by-run via `longestEffectiveRange`
    /// and compares each pair with tolerant comparators (M4 plan item 3):
    /// font by name + point size (±0.01), everything else (kern, color,
    /// strikethrough, ...) via `NSObject.isEqual`, `.paragraphStyle` skipped
    /// when neither side sets it (`DefaultKamiTheme` never sets it; a custom
    /// theme that does still gets the `isEqual` comparison).
    private func firstAttributeMismatch(live: NSAttributedString, oracle: NSAttributedString) -> String? {
        let length = live.length
        var location = 0
        while location < length {
            var liveRange = NSRange()
            var oracleRange = NSRange()
            let liveAttrs = live.attributes(
                at: location, longestEffectiveRange: &liveRange, in: NSRange(location: location, length: length - location))
            let oracleAttrs = oracle.attributes(
                at: location, longestEffectiveRange: &oracleRange, in: NSRange(location: location, length: length - location))
            if !attributesTolerantlyEqual(liveAttrs, oracleAttrs) {
                return "attribute mismatch at \(location): live=\(liveAttrs) oracle=\(oracleAttrs)"
            }
            location += max(1, min(liveRange.length, oracleRange.length))
        }
        return nil
    }

    private func attributesTolerantlyEqual(_ a: [NSAttributedString.Key: Any], _ b: [NSAttributedString.Key: Any]) -> Bool {
        let keys = Set(a.keys).union(b.keys)
        for key in keys {
            if key == .paragraphStyle, a[key] == nil, b[key] == nil { continue }
            if key == .font {
                let fontA = a[key] as? NSFont
                let fontB = b[key] as? NSFont
                guard let fontA, let fontB else { return fontA == nil && fontB == nil }
                if abs(fontA.pointSize - fontB.pointSize) > 0.01 { return false }
                if fontA.fontName == fontB.fontName { continue }
                // Apple's private system/cascade meta-fonts (name prefixed
                // "."; `.systemFont(ofSize:)` returns `.AppleSystemUIFont`)
                // resolve per-glyph at CoreText layout time regardless of
                // which concrete substitute (e.g. `.AppleColorEmojiUI` for
                // an emoji run) is literally stored in the attribute —
                // whether that substitution already happened depends only
                // on which apply path last touched the run (`setAttributedString`
                // triggers `NSTextStorage`'s automatic `fixAttributes`;
                // `setAttributes` does not), not on anything the theme
                // requested. Both render identically (verified: `.AppleSystemUIFont`
                // is not itself glyph-complete for emoji: `CTFontGetGlyphsForCharacters`
                // fails on it directly, yet a real `NSLayoutManager` still
                // resolves a glyph without `fixAttributes` ever running —
                // this is exactly the class of divergence that
                // `KamiTextStorage.fixAttributes` — a deliberately deferred
                // non-goal — already accounts for). The fixing pass can also
                // substitute a concrete public font (e.g. Helvetica for a text-presentation
                // dingbat the meta-font lacks — surfaced by the corpus stress
                // doc, see PLATFORM_BUGS.md #4), so the tolerance is: same
                // size AND at least one side is a dot-prefixed meta-font —
                // the meta-font side proves the theme asked for the system
                // cascade and the other side is its post-fix resolution.
                // Two differently-named concrete fonts still fail.
                if fontA.fontName.hasPrefix(".") || fontB.fontName.hasPrefix(".") { continue }
                return false
            }
            let valueA = a[key] as AnyObject?
            let valueB = b[key] as AnyObject?
            switch (valueA, valueB) {
            case (nil, nil): continue
            case let (lhs?, rhs?): if !lhs.isEqual(rhs) { return false }
            default: return false
            }
        }
        return true
    }
}
#endif
