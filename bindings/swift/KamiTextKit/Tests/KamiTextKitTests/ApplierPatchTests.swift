import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Regression: a checkbox toggle dirties only the marker bytes, but the
/// checked-task strikethrough overlay lives on the item's CONTENT — the
/// applier must widen dirty ranges to whole task elements or the overlay
/// goes stale in both directions (check and uncheck).
@MainActor
struct ApplierPatchTests {
    @Test func checkedTaskOverlayFollowsToggle() throws {
        let text = "- [ ] todo\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)

        let contentLocation = (text as NSString).range(of: "todo").location
        #expect(strikethrough(storage, at: contentLocation) == false)

        // Toggle to checked: mirror the plan into storage, then patch-apply.
        try toggle(engine: engine, applier: applier, storage: storage)
        #expect(strikethrough(storage, at: contentLocation) == true)

        // Toggle back: the overlay must clear again.
        try toggle(engine: engine, applier: applier, storage: storage)
        #expect(strikethrough(storage, at: contentLocation) == false)
    }

    private func toggle(engine: KamiEngine, applier: KamiTextStorageApplier, storage: NSTextStorage) throws {
        let plan = try #require(try engine.toggleTaskPlan(at: 0))
        for edit in plan.edits {
            // Convert byte range → UTF-16 against the PRE-edit engine state.
            let loc = try engine.byteToUtf16(edit.range.lowerBound)
            let end = try engine.byteToUtf16(edit.range.upperBound)
            let patch = try engine.applyEdit(edit.range, replacement: edit.text)
            storage.replaceCharacters(
                in: NSRange(location: Int(loc), length: Int(end - loc)),
                with: edit.text
            )
            try applier.apply(patch, engine: engine, to: storage)
        }
        #expect(engine.lenUtf16 == UInt32(storage.length))
    }

    /// A custom theme's `checkedTaskOverlayAttributes()` must be honored — the
    /// applier reads it from the theme, not a hardcoded constant.
    @Test func customThemeOverlayIsApplied() throws {
        let text = "- [x] done\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier(theme: BackgroundOverlayTheme())
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)

        let contentLocation = (text as NSString).range(of: "done").location
        let background = storage.attribute(.backgroundColor, at: contentLocation, effectiveRange: nil)
        #expect(background != nil, "custom overlay attribute (backgroundColor) not applied")
        // The default strikethrough overlay must NOT appear — the custom theme
        // replaced it wholesale.
        #expect(strikethrough(storage, at: contentLocation) == false)
    }

    @Test func checkedParentDoesNotStrikeNestedChildren() throws {
        // The parent element's range swallows the nested sublist, but checked
        // state does not cascade: the child keeps its own (unchecked) face,
        // and the child line's leading indent gets no struck whitespace stub.
        let text = "- [x] parent line\n    - [ ] child line\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)
        let ns = text as NSString
        #expect(strikethrough(storage, at: ns.range(of: "parent").location) == true)
        #expect(strikethrough(storage, at: ns.range(of: "child").location) == false)
        #expect(strikethrough(storage, at: ns.range(of: "    -").location) == false)
    }

    @Test func checkedChildStrikesOnlyItself() throws {
        // The inverse direction: a checked child under an unchecked parent
        // strikes its own content and nothing of the parent's.
        let text = "- [ ] parent line\n    - [x] child line\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)
        let ns = text as NSString
        #expect(strikethrough(storage, at: ns.range(of: "parent").location) == false)
        #expect(strikethrough(storage, at: ns.range(of: "child").location) == true)
    }

    @Test func checkedChildWithFollowingSiblingStillStrikes() throws {
        // A nested item's body run coalesces into the NEXT sibling's leading
        // indent, so the checked child's own segment spills past its element —
        // the line-window overlay must still strike its text (a containment
        // check would drop it) and must not touch the sibling.
        let text = "- [ ] parent line\n    - [x] child one\n    - [ ] child two\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)
        let ns = text as NSString
        #expect(strikethrough(storage, at: ns.range(of: "child one").location) == true)
        #expect(strikethrough(storage, at: ns.range(of: "child two").location) == false)
    }

    @Test func checkedTaskDoesNotStrikePastBlankLine() throws {
        // A blank line ends the item (CommonMark), but the following plain
        // paragraph coalesces into the same body segment — the blank-line cap
        // must keep the overlay off it.
        let text = "- [x] done line\n\nfollowing paragraph\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)
        let ns = text as NSString
        #expect(strikethrough(storage, at: ns.range(of: "done").location) == true)
        #expect(strikethrough(storage, at: ns.range(of: "following").location) == false)
    }

    private func strikethrough(_ storage: NSTextStorage, at location: Int) -> Bool {
        storage.attribute(.strikethroughStyle, at: location, effectiveRange: nil) != nil
    }

    /// Phase 2.2 regression: a dirty range strictly between two task
    /// elements must not widen to either — only task elements the dirty
    /// range actually intersects should trigger widening (`intersects` in
    /// `KamiTextStorageApplier.swift`).
    @Test func dirtyRangeBetweenTasksDoesNotWidenToNeighboringTasks() throws {
        let text = "- [ ] task one\n\nplain paragraph\n\n- [x] task two\n"
        let engine = try KamiEngine(text: text)
        let applier = KamiTextStorageApplier()
        let storage = NSTextStorage(string: text)
        try applier.applyFull(engine: engine, to: storage)

        // The task-marker prefixes ("- [ ] " / "- [x] ") are the only bytes
        // genuinely specific to a task element and distinct in kind from
        // ordinary body text; a widened `applyRange` would pull a marker
        // segment into the second apply's fetch and restyle it. Ordinary
        // body-segment coalescing across the blank lines (segments are
        // maximal same-kind runs, so a fetch for "plain paragraph" alone
        // naturally overlaps some neighboring body text on both sides) is
        // expected and unrelated to task widening, so this checks marker
        // bytes specifically rather than the tasks' full element ranges.
        let markerRanges = try engine.segments(in: 0..<engine.lenBytes)
            .filter { $0.kinds.contains(.taskMarker) }
            .map { segment -> NSRange in
                let loc = try engine.byteToUtf16(segment.range.lowerBound)
                let end = try engine.byteToUtf16(segment.range.upperBound)
                return NSRange(location: Int(loc), length: Int(end - loc))
            }
        #expect(markerRanges.count == 2)

        let paragraphNSRange = (text as NSString).range(of: "plain paragraph")
        let byteStart = try engine.utf16ToByte(UInt32(paragraphNSRange.location))
        let byteEnd = try engine.utf16ToByte(UInt32(paragraphNSRange.location + paragraphNSRange.length))

        let spy = EditRangeSpy()
        storage.delegate = spy
        try applier.apply(KamiPatch(dirty: [byteStart..<byteEnd]), engine: engine, to: storage)

        #expect(!spy.editedRanges.isEmpty)
        for edited in spy.editedRanges {
            for markerRange in markerRanges {
                #expect(NSIntersectionRange(edited, markerRange).length == 0, "restyle touched a neighboring task's marker")
            }
        }
    }
}

/// Custom theme: default base styling, but the checked-task overlay is a
/// background highlight instead of strikethrough (proves the overlay hook).
private struct BackgroundOverlayTheme: KamiTheme {
    private let base = DefaultKamiTheme()
    func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
        base.attributes(for: kinds, concealed: concealed)
    }
    func checkedTaskOverlayAttributes() -> [NSAttributedString.Key: Any] {
        [.backgroundColor: KamiColor.systemYellow]
    }
}

#if canImport(UIKit)
private typealias StorageEditActions = NSTextStorage.EditActions
#else
private typealias StorageEditActions = NSTextStorageEditActions
#endif

/// Records the storage ranges touched by each `beginEditing`/`endEditing`
/// batch, so a test can assert exactly what a patch-apply restyled.
private final class EditRangeSpy: NSObject, NSTextStorageDelegate {
    var editedRanges: [NSRange] = []

    func textStorage(
        _ textStorage: NSTextStorage, didProcessEditing editedMask: StorageEditActions,
        range editedRange: NSRange, changeInLength delta: Int
    ) {
        editedRanges.append(editedRange)
    }
}
