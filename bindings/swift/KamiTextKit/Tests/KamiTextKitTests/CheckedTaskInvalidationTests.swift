import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Regression for a fuzz-found `KamiTextSync` adapter-state bug: an edit
/// that destroys, deletes, or shrinks a checked task can leave stale
/// checked-task overlay (strikethrough + tertiary color) on content OUTSIDE
/// both the engine's own dirty range and any post-edit task element.
/// `KamiTextStorageApplier.apply`'s dirty-range widening (see its doc
/// comment, `KamiTextStorageApplier.swift`) only ever sees POST-edit
/// `elements(in:)` — a task the edit destroyed or shrunk away has nothing
/// left there to widen onto. `KamiTextSync` alone sees both sides of the
/// edit, so it tracks checked-task ranges itself and repaints whatever an
/// edit invalidates (`checkedTaskByteRanges` / `reconcileCheckedTaskOverlays`
/// in `KamiTextSync.swift`).
@MainActor
struct CheckedTaskInvalidationTests {
    /// Minimized fuzz repro: inserting a single character into the list
    /// marker ("- " → "-X ") breaks the task syntax entirely. The checked
    /// task vanishes, but "**bold**" — part of the item's lazy-continued
    /// content — carried the strikethrough+tertiary overlay from the
    /// pre-edit checked task; it must clear.
    @Test func taskDestroyedByMarkerBreakingInsertClearsStaleOverlay() throws {
        let text = "- [x] done\nbody text **bold** more text\n"
        let (sync, storage) = seededSync(text: text)
        try #require(try hasCheckedTask(sync))

        applyEdit(sync, storage: storage, range: NSRange(location: 1, length: 0), replacement: "X")

        #expect(try !hasCheckedTask(sync))
        #expect(strikethrough(storage, at: boldLocation(in: storage)) == false)
        try assertSynced(sync, storage: storage)
    }

    /// Deleting the checkbox token itself (`[x]` removed) destroys the task
    /// via a different edit shape — same stale-overlay gap.
    @Test func taskDestroyedByDeletingCheckboxClearsStaleOverlay() throws {
        let text = "- [x] done\nbody **bold** text\n"
        let (sync, storage) = seededSync(text: text)
        try #require(try hasCheckedTask(sync))

        applyEdit(sync, storage: storage, range: NSRange(location: 2, length: 3), replacement: "")

        #expect(try !hasCheckedTask(sync))
        #expect(strikethrough(storage, at: boldLocation(in: storage)) == false)
        try assertSynced(sync, storage: storage)
    }

    /// Unchecking (`[x]` → `[ ]`) dirties only the checkbox byte, but the
    /// task element itself survives (just `checked = false`) —
    /// `KamiTextStorageApplier.apply`'s own dirty-range widening already
    /// finds that surviving element and repaints its content, so this
    /// should already pass without the new reconciliation doing anything.
    @Test func taskUncheckedClearsOverlayViaExistingWidening() throws {
        let text = "- [x] first line\ncontinued lazy text with **bold** more\n"
        let (sync, storage) = seededSync(text: text)
        let location = boldLocation(in: storage)
        #expect(strikethrough(storage, at: location) == true)

        applyEdit(sync, storage: storage, range: NSRange(location: 3, length: 1), replacement: " ")

        #expect(strikethrough(storage, at: location) == false)
        try assertSynced(sync, storage: storage)
    }

    /// Breaking a lazy continuation (a blank line before a 0-indent tail
    /// that no longer meets the list item's continuation width) shrinks the
    /// task down to just its first line. The tail — with "**bold**" —
    /// survives as a plain paragraph outside the now-smaller task range and
    /// must lose its overlay.
    @Test func taskRangeShrunkByBrokenLazyContinuationClearsTailOverlay() throws {
        let text = "- [x] first line\ncontinued lazy text with **bold** more\n"
        let (sync, storage) = seededSync(text: text)
        #expect(strikethrough(storage, at: boldLocation(in: storage)) == true)

        let insertAt = (text as NSString).range(of: "continued").location
        applyEdit(sync, storage: storage, range: NSRange(location: insertAt, length: 0), replacement: "\n")

        #expect(strikethrough(storage, at: boldLocation(in: storage)) == false)
        try assertSynced(sync, storage: storage)
    }

    // MARK: - Helpers

    private func seededSync(text: String) -> (KamiTextSync, NSTextStorage) {
        let storage = NSTextStorage()
        let sync = KamiTextSync()
        sync.seed(text: text, storage: storage, selectedRange: NSRange(location: 0, length: 0))
        return (sync, storage)
    }

    /// Mirrors the willChange/didChange bracket a host's edit delegates
    /// drive (`KamiDemoMac`'s `runSelftest`, `FuzzHost`) — `willChange`
    /// stashes against the PRE-edit engine, the caller then mutates
    /// `storage` the way the platform would, and `didChange` applies it.
    private func applyEdit(_ sync: KamiTextSync, storage: NSTextStorage, range: NSRange, replacement: String) {
        sync.willChange(range: range, replacement: replacement, storageLength: storage.length, isComposing: false)
        storage.replaceCharacters(in: range, with: replacement)
        sync.didChange(
            text: storage.string,
            storage: storage,
            selectedRange: NSRange(location: range.location + (replacement as NSString).length, length: 0),
            isComposing: false
        )
    }

    private func hasCheckedTask(_ sync: KamiTextSync) throws -> Bool {
        let engine = try #require(sync.engine)
        return try engine.elements(in: 0..<engine.lenBytes).contains { $0.kind == .task && $0.checked }
    }

    private func boldLocation(in storage: NSTextStorage) -> Int {
        (storage.string as NSString).range(of: "bold").location
    }

    private func strikethrough(_ storage: NSTextStorage, at location: Int) -> Bool {
        storage.attribute(.strikethroughStyle, at: location, effectiveRange: nil) != nil
    }

    private func assertSynced(_ sync: KamiTextSync, storage: NSTextStorage) throws {
        let engine = try #require(sync.engine)
        #expect(engine.lenUtf16 == UInt32(storage.length))
    }
}
