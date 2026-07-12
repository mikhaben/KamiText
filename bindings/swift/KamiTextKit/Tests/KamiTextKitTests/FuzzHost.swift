import Foundation
import Testing
@testable import KamiTextKit
#if os(macOS)
import AppKit

/// `NSTextView` subclass hosting the fuzz shim. Rich-text features are
/// disabled so fuzz-generated markdown lands byte-for-byte as generated —
/// mirrors `KamiDemoMac`'s `KamiDemoTextView` markdown-source configuration
/// (`Sources/KamiDemoMac/main.swift`).
@MainActor
final class FuzzHostTextView: NSTextView {}

/// Test-local host shim (M4 plan item 1): a real TextKit 2 `NSTextView` +
/// `KamiTextSync`, wired the way `KamiDemoMac` wires it — the three
/// `NSTextViewDelegate` hooks (plural `shouldChangeTextInRanges`,
/// `textDidChange`, `textViewDidChangeSelection`) — plus the
/// `NSTextStorageDelegate.didProcessEditing` reseed the `KamiTextSync`
/// header prescribes for bulletproof undo coverage: undo/redo fires
/// neither edit hook (header caveat, `KamiTextSync.swift`), only a
/// selection change, so a length-preserving undo would otherwise slip past
/// `selectionChanged`'s own length-mismatch recovery.
///
/// `isSyncManaged` distinguishes a character edit the normal recipe already
/// owns — the `shouldChangeTextInRanges` → `textDidChange` bracket, or an
/// explicit `seed` call — from one it never sees (undo/redo): only the
/// latter reseeds from `didProcessEditing`. This also guards against
/// reentering `seed` from within its own callback: `KamiTextStorageApplier
/// .applyFull` (called by `seed`) applies via `NSTextStorage
/// .setAttributedString`, which DOES edit characters (unlike the
/// patch-driven `apply`, which only sets attributes) and so re-fires
/// `didProcessEditing` synchronously during `seed` itself.
@MainActor
final class FuzzHost: NSObject, NSTextViewDelegate {
    let sync: KamiTextSync
    let view: FuzzHostTextView
    let storage: NSTextStorage
    private let window: NSWindow

    private var isSyncManaged = false

    init(text: String, options: KamiEngine.Options) {
        sync = KamiTextSync(options: options)
        view = FuzzHostTextView(usingTextLayoutManager: true)
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 300),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        // A freshly constructed NSTextView always has a text storage.
        storage = view.textStorage!
        super.init()

        // Markdown source editing: rich-text commands and smart
        // substitutions would rewrite fuzz-generated fragments out from
        // under the deterministic seed.
        view.isRichText = false
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false
        view.allowsUndo = true
        view.delegate = self
        storage.delegate = self

        window.contentView = view
        window.makeFirstResponder(view)

        isSyncManaged = true
        view.string = text
        // Seed with the view's ACTUAL selection (KamiDemoMac's pattern), not
        // an assumed (0, 0) — `string =` is free to place the caret wherever
        // AppKit likes, and seeding with a selection the view doesn't hold
        // would desync the two from the very first op.
        sync.seed(text: text, storage: storage, selectedRange: view.selectedRange())
        isSyncManaged = false
    }

    // MARK: - NSTextViewDelegate -> KamiTextSync (KamiDemoMac wiring shape)

    func textView(
        _ textView: NSTextView,
        shouldChangeTextInRanges affectedRanges: [NSValue],
        replacementStrings: [String]?
    ) -> Bool {
        isSyncManaged = true
        if let replacementStrings,
           affectedRanges.count == 1,
           let range = affectedRanges.first?.rangeValue,
           let replacement = replacementStrings.first {
            sync.willChange(
                range: range,
                replacement: replacement,
                storageLength: storage.length,
                isComposing: textView.hasMarkedText()
            )
        }
        return true
    }

    func textDidChange(_ notification: Notification) {
        defer { isSyncManaged = false }
        sync.didChange(
            text: storage.string,
            storage: storage,
            selectedRange: view.selectedRange(),
            isComposing: view.hasMarkedText()
        )
    }

    func textViewDidChangeSelection(_ notification: Notification) {
        sync.selectionChanged(
            selectedRange: view.selectedRange(),
            text: storage.string,
            storage: storage,
            isComposing: view.hasMarkedText()
        )
    }

    /// Content compare first (length-preserving strands/undos are the
    /// common case — see `KamiTextSync.recoverIfDesynced`), length check as
    /// throw-fallback.
    private func isDesynced(against textStorage: NSTextStorage) -> Bool {
        guard let engine = sync.engine else { return true }
        if let engineText = try? engine.text() {
            return engineText != textStorage.string
        }
        return engine.lenUtf16 != UInt32(textStorage.length)
    }
}

// MARK: - Bulletproof undo (KamiTextSync header recipe)
//
// @preconcurrency: the delegate protocol is nonisolated, but AppKit only
// calls it on the thread mutating the storage — always main here
// (runtime-checked).
extension FuzzHost: @preconcurrency NSTextStorageDelegate {
    /// Fires for every character mutation regardless of which (if any)
    /// `NSTextViewDelegate` edit hook ran — undo/redo included. Reseeding
    /// only when `isSyncManaged` is false (the mutation happened outside
    /// the normal recipe) and the engine is actually desynced keeps this a
    /// no-op for ordinary keystrokes, where `textDidChange` is about to run
    /// its own incremental `applyEdit`.
    func textStorage(
        _ textStorage: NSTextStorage,
        didProcessEditing editedMask: NSTextStorageEditActions,
        range editedRange: NSRange,
        changeInLength delta: Int
    ) {
        guard editedMask.contains(.editedCharacters), !isSyncManaged else { return }
        guard isDesynced(against: textStorage) else { return }
        isSyncManaged = true
        sync.seed(text: textStorage.string, storage: textStorage, selectedRange: view.selectedRange())
        isSyncManaged = false
    }
}
#endif
