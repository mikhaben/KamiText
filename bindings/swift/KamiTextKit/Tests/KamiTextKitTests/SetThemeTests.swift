import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// `KamiTextSync.setTheme` contract: an attribute-only restyle. No character
/// edits reach the storage (the invariant dirty-tracking hosts and undo depend
/// on), the text is untouched, and the sync keeps styling subsequent edits
/// with the new theme.
@MainActor
struct SetThemeTests {
    private static let doc = "# Head\n\nBody **bold** text"
    /// UTF-16 index of the "B" in "Body" — plain body text, no markers.
    private static let bodyIndex = 8

    @Test func setThemeRestylesWithoutCharacterEdits() throws {
        let sync = KamiTextSync(theme: DefaultKamiTheme())
        let storage = NSTextStorage()
        sync.seed(text: Self.doc, storage: storage, selectedRange: NSRange(location: 0, length: 0))
        let before = storage.string

        let spy = EditSpy()
        storage.delegate = spy
        try sync.setTheme(HotPinkTheme(), storage: storage)

        #expect(storage.string == before)
        #expect(spy.characterEdits == 0)
        #expect(spy.attributeOnlyEdits > 0)

        let color = storage.attribute(.foregroundColor, at: Self.bodyIndex, effectiveRange: nil) as? KamiColor
        #expect(color == HotPinkTheme.bodyColor)
    }

    @Test func keystrokeAfterSetThemeUsesNewTheme() throws {
        let sync = KamiTextSync(theme: DefaultKamiTheme())
        let storage = NSTextStorage()
        sync.seed(text: Self.doc, storage: storage, selectedRange: NSRange(location: 0, length: 0))
        try sync.setTheme(HotPinkTheme(), storage: storage)

        // Simulate the host recipe for one keystroke appended at the end.
        let at = storage.length
        sync.willChange(
            range: NSRange(location: at, length: 0), replacement: "x",
            storageLength: storage.length, isComposing: false
        )
        storage.replaceCharacters(in: NSRange(location: at, length: 0), with: "x")
        sync.didChange(
            text: storage.string, storage: storage,
            selectedRange: NSRange(location: at + 1, length: 0), isComposing: false
        )

        #expect(storage.string == Self.doc + "x")
        let color = storage.attribute(.foregroundColor, at: at, effectiveRange: nil) as? KamiColor
        #expect(color == HotPinkTheme.bodyColor)
    }

    @Test func setThemeBeforeSeedIsSafeAndSeedsWithNewTheme() throws {
        let sync = KamiTextSync(theme: DefaultKamiTheme())
        let storage = NSTextStorage()

        // No engine yet: swaps the applier, touches nothing.
        try sync.setTheme(HotPinkTheme(), storage: storage)
        #expect(storage.length == 0)

        sync.seed(text: Self.doc, storage: storage, selectedRange: NSRange(location: 0, length: 0))
        let color = storage.attribute(.foregroundColor, at: Self.bodyIndex, effectiveRange: nil) as? KamiColor
        #expect(color == HotPinkTheme.bodyColor)
    }

    @Test func setThemeOnEmptyDocumentIsANoOp() throws {
        let sync = KamiTextSync(theme: DefaultKamiTheme())
        let storage = NSTextStorage()
        sync.seed(text: "", storage: storage, selectedRange: NSRange(location: 0, length: 0))
        try sync.setTheme(HotPinkTheme(), storage: storage)
        #expect(storage.length == 0)
    }
}

/// Distinctive theme so restyles are unambiguous in assertions.
private struct HotPinkTheme: KamiTheme {
    static let bodyColor = KamiColor.systemPink

    func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
        if concealed {
            return [
                .font: KamiFont.systemFont(ofSize: 0.01),
                .foregroundColor: KamiColor.clear,
                .kern: -0.01,
            ]
        }
        return [
            .font: KamiFont.systemFont(ofSize: 15),
            .foregroundColor: Self.bodyColor,
        ]
    }
}

#if canImport(UIKit)
private typealias StorageEditActions = NSTextStorage.EditActions
#else
private typealias StorageEditActions = NSTextStorageEditActions
#endif

private final class EditSpy: NSObject, NSTextStorageDelegate {
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
