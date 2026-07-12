import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// The recovery-reseed contract that keeps the platform caret alive: when the
/// storage already holds the correct text (every desync recovery — the
/// storage is the source of truth, only the engine is stale), `seed` must
/// restyle in place with zero character edits. A whole-storage
/// `setAttributedString` at that moment is what resets UIKit's caret
/// rendering and undo coalescing.
@MainActor
struct CaretRecoveryTests {
    private static let doc = "# Head\n\nBody **bold** text\n"

    /// The iOS-undo shape: engine goes stale while the storage is correct
    /// (undo fired `textViewDidChange` with no `shouldChangeTextIn` stash).
    /// The recovery reseed must be attribute-only.
    @Test func desyncRecoveryReseedIsAttributeOnly() throws {
        let sync = KamiTextSync()
        let storage = NSTextStorage()
        sync.seed(text: Self.doc, storage: storage, selectedRange: NSRange(location: 0, length: 0))

        // Simulate the platform mutating the storage behind the sync's back
        // (what an undo does): the storage is now truth, the engine is stale.
        storage.replaceCharacters(in: NSRange(location: storage.length - 1, length: 0), with: "undone")
        #expect(sync.engine!.lenUtf16 != UInt32(storage.length))

        let spy = EditKindSpy()
        storage.delegate = spy
        let restyled = sync.selectionChanged(
            selectedRange: NSRange(location: 0, length: 0),
            text: storage.string, storage: storage, isComposing: false
        )

        #expect(restyled, "length desync must trigger a recovery reseed")
        #expect(sync.engine!.lenUtf16 == UInt32(storage.length), "reseed must resync the engine")
        #expect(spy.characterEdits == 0, "recovery reseed must never edit characters")
        #expect(spy.attributeOnlyEdits > 0, "recovery reseed must restyle")
    }

    /// Initial load into an empty storage still takes the character-
    /// populating path.
    @Test func seedIntoEmptyStoragePopulatesText() throws {
        let sync = KamiTextSync()
        let storage = NSTextStorage()
        sync.seed(text: Self.doc, storage: storage, selectedRange: NSRange(location: 0, length: 0))
        #expect(storage.string == Self.doc)
    }

    /// `selectionChanged` reports whether it restyled: crossing into another
    /// line flips reveal state (true); repeating the same caret is a no-op
    /// (false). UIKit hosts key their caret-refresh nudge off this.
    @Test func selectionChangedReportsRestyles() throws {
        let sync = KamiTextSync()
        let storage = NSTextStorage()
        sync.seed(text: Self.doc, storage: storage, selectedRange: NSRange(location: 0, length: 0))

        let bodyLine = (Self.doc as NSString).range(of: "Body").location
        let moved = sync.selectionChanged(
            selectedRange: NSRange(location: bodyLine, length: 0),
            text: storage.string, storage: storage, isComposing: false
        )
        #expect(moved, "crossing lines flips reveal state and must report a restyle")

        let repeated = sync.selectionChanged(
            selectedRange: NSRange(location: bodyLine, length: 0),
            text: storage.string, storage: storage, isComposing: false
        )
        #expect(!repeated, "an identical selection is a no-op and must not report a restyle")
    }
}

#if canImport(UIKit)
private typealias StorageEditActions = NSTextStorage.EditActions
#else
private typealias StorageEditActions = NSTextStorageEditActions
#endif

private final class EditKindSpy: NSObject, NSTextStorageDelegate {
    var characterEdits = 0
    var attributeOnlyEdits = 0

    func textStorage(
        _ textStorage: NSTextStorage, didProcessEditing editedMask: StorageEditActions,
        range editedRange: NSRange, changeInLength delta: Int
    ) {
        if editedMask.contains(.editedCharacters) {
            characterEdits += 1
        } else {
            attributeOnlyEdits += 1
        }
    }
}
