import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// The applier sets a per-paragraph `headIndent` on bullet/ordinal list items so
/// soft-wrapped continuation lines hang under the item text (`firstLineHeadIndent`
/// stays 0). `DefaultKamiTheme` emits NO paragraph style, so any `headIndent`
/// present is the list pass's doing — making these assertions deterministic
/// without depending on a theme's metrics.
@MainActor
struct ListIndentTests {
    @Test func topLevelBulletHangs() throws {
        let storage = try seed("- item text that wraps\n")
        // "item" content sits at index 2 (after "- ").
        #expect(headIndent(storage, at: 2) > 0)
        #expect(firstLineHeadIndent(storage, at: 2) == 0)
    }

    @Test func orderedWidthScales() throws {
        // "10. " is wider than "1. ", so its hanging indent must be larger.
        let storage = try seed("1. a\n10. b\n")
        let oneDot = headIndent(storage, at: 0)   // "1." ordinal
        let tenDot = headIndent(storage, at: 5)   // "10." ordinal (line 2 starts at byte 5)
        #expect(oneDot > 0)
        #expect(tenDot > oneDot)
    }

    @Test func nestedBulletDeeperThanTopLevel() throws {
        // "  - sub" carries 2 leading spaces, so it hangs further than "- top".
        let storage = try seed("- top\n  - sub\n")
        let top = headIndent(storage, at: 0)   // top bullet
        let sub = headIndent(storage, at: 8)   // nested bullet (after "\n  ")
        #expect(top > 0)
        #expect(sub > top)
    }

    @Test func taskItemUntouched() throws {
        // Task markers carry `.taskMarker`, not `.listBullet`/`.listOrdinal`, so
        // the list pass must skip them (DefaultKamiTheme leaves no indent).
        let storage = try seed("- [ ] todo\n")
        let todo = (storage.string as NSString).range(of: "todo").location
        #expect(headIndent(storage, at: todo) == 0)
    }

    @Test func nestedTaskKeepsBoxReserve() throws {
        // A NESTED concealed task's paragraph begins with body-styled indent
        // whitespace, so paragraph fixing would propagate BODY over the marker
        // run's task style and drop the checkbox reserve. The pass re-derives
        // it explicitly: first line = the theme's reserve, wrapped lines hang
        // under the text (reserve + whitespace prefix width).
        let text = "x\n- [ ] top task\n    - [ ] sub task\n"   // caret on "x": both concealed
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier(theme: TaskIndentTheme())
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)
        let sub = (storage.string as NSString).range(of: "sub task").location
        #expect(abs(firstLineHeadIndent(storage, at: sub) - TaskIndentTheme.reserve) < 0.5)
        #expect(headIndent(storage, at: sub) > TaskIndentTheme.reserve)
    }

    @Test func idempotentOnReapply() throws {
        let engine = try KamiEngine(text: "- item text\n")
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: try engine.text())
        try applier.applyFull(engine: engine, to: storage)
        let first = headIndent(storage, at: 2)
        try applier.applyFull(engine: engine, to: storage)
        let second = headIndent(storage, at: 2)
        #expect(first > 0)
        #expect(abs(first - second) < 0.01)
    }

    @Test func patchPathProducesHangingIndent() throws {
        // Not only applyFull: a live edit that turns a plain line into a list
        // item must produce the hanging indent through the patch path.
        let engine = try KamiEngine(text: "x\n")
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: try engine.text())
        try applier.applyFull(engine: engine, to: storage)
        #expect(headIndent(storage, at: 0) == 0)

        let range: Range<UInt32> = 0..<0
        let patch = try engine.applyEdit(range, replacement: "- ")
        storage.replaceCharacters(in: NSRange(location: 0, length: 0), with: "- ")
        try applier.apply(patch, engine: engine, to: storage)

        #expect(engine.lenUtf16 == UInt32(storage.length))
        #expect(headIndent(storage, at: 2) > 0)      // "x" now at index 2
        #expect(firstLineHeadIndent(storage, at: 2) == 0)
    }

    @Test func nestedIndentSurvivesEditInPreviousItem() throws {
        // Regression guard: the coalesced BODY segment spanning
        // "top\n  " carries the NESTED paragraph's leading whitespace. An edit
        // inside "top" dirties that segment; the per-segment reset then wrote
        // headIndent-0 over the nested paragraph's first character (paragraph
        // fixing propagates it) while the nested BULLET segment stayed outside
        // the dirty range — wiping the indent with no self-heal. The patch
        // path's paragraph widening keeps the marker in the pass.
        let engine = try KamiEngine(text: "- top\n  - sub item\n")
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: try engine.text())
        try applier.applyFull(engine: engine, to: storage)
        let before = headIndent(storage, at: 10)   // "sub item" content
        #expect(before > 0)

        // Insert at the end of "top" (byte 5) — first paragraph's body.
        let at: UInt32 = 5
        let patch = try engine.applyEdit(at..<at, replacement: "x")
        storage.replaceCharacters(in: NSRange(location: 5, length: 0), with: "x")
        try applier.apply(patch, engine: engine, to: storage)

        #expect(engine.lenUtf16 == UInt32(storage.length))
        // Content shifted by 1; the nested indent must survive unchanged.
        #expect(abs(headIndent(storage, at: 11) - before) < 0.01)
    }

    @Test func healsWhenNoLongerList() throws {
        let engine = try KamiEngine(text: "- x\n")
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: try engine.text())
        try applier.applyFull(engine: engine, to: storage)
        #expect(headIndent(storage, at: 2) > 0)

        // Delete "- " (bytes 0..<2) → "x\n": the paragraph is no longer a list
        // and the hanging indent must clear (self-healing, no sentinel).
        let range: Range<UInt32> = 0..<2
        let patch = try engine.applyEdit(range, replacement: "")
        storage.replaceCharacters(in: NSRange(location: 0, length: 2), with: "")
        try applier.apply(patch, engine: engine, to: storage)

        #expect(engine.lenUtf16 == UInt32(storage.length))
        #expect(headIndent(storage, at: 0) == 0)
    }

    // MARK: - Helpers

    private func seed(_ text: String) throws -> NSTextStorage {
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: try engine.text())
        try applier.applyFull(engine: engine, to: storage)
        return storage
    }

    private func headIndent(_ storage: NSTextStorage, at location: Int) -> CGFloat {
        (storage.attribute(.paragraphStyle, at: location, effectiveRange: nil) as? NSParagraphStyle)?.headIndent ?? 0
    }

    /// `DefaultKamiTheme` plus a checkbox reserve on concealed task markers —
    /// the minimal theme shape the task-indent branch keys off.
    private struct TaskIndentTheme: KamiTheme {
        static let reserve: CGFloat = 24
        func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
            var attrs = DefaultKamiTheme().attributes(for: kinds, concealed: concealed)
            if kinds.contains(.taskMarker), concealed {
                let style = NSMutableParagraphStyle()
                style.firstLineHeadIndent = Self.reserve
                style.headIndent = Self.reserve
                attrs[.paragraphStyle] = style.copy()
            }
            return attrs
        }
    }

    private func firstLineHeadIndent(_ storage: NSTextStorage, at location: Int) -> CGFloat {
        (storage.attribute(.paragraphStyle, at: location, effectiveRange: nil) as? NSParagraphStyle)?.firstLineHeadIndent ?? 0
    }
}
