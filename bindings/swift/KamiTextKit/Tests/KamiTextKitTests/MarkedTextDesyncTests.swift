import Foundation
import Testing
@testable import KamiTextKit
#if os(macOS)
import AppKit

/// Test-local host shim implementing the `KamiTextSync` header's AppKit
/// stranded-composition recovery recipe (`becomeFirstResponder` →
/// `unmarkText` → `recoverIfDesynced`) — this proves the documented recipe
/// against a real `NSTextView`/`NSWindow`, not just `KamiTextSync` in
/// isolation. No edit/selection delegate is wired: these tests only exercise
/// the recovery path, not general keystroke sync (covered elsewhere).
@MainActor
private final class DesyncHostTextView: NSTextView {
    let sync = KamiTextSync()

    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became, let storage = textStorage {
            if hasMarkedText() { unmarkText() }
            _ = sync.recoverIfDesynced(text: storage.string, storage: storage, selectedRange: selectedRange())
        }
        return became
    }
}

/// Stranded-composition recovery (PLATFORM_BUGS.md #3): a stranded
/// IME/inline-prediction composition leaves the engine's model frozen while
/// the text view keeps mutating ("delete drift"); focus regain is the
/// recovery signal. `.serialized`: tests share real `NSWindow` first-responder state.
@Suite("Marked-text desync recovery", .serialized)
@MainActor
struct MarkedTextDesyncTests {
    @Test func regainingFocusRecoversStrandedComposition() throws {
        let (window, view) = makeHost(text: "alpha bravo charlie")
        let storage = try #require(view.textStorage)
        let engine = try #require(view.sync.engine)
        #expect(try engine.text() == storage.string)

        // Strand a composition: marked text lands in storage, the engine is
        // never told (no edit delegate is wired in this shim).
        view.setMarkedText(
            "´",
            selectedRange: NSRange(location: 0, length: 1),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        #expect(try engine.text() != storage.string)

        // Resign then regain first responder — the out-of-band recovery signal.
        window.makeFirstResponder(nil)
        _ = window.makeFirstResponder(view)

        #expect(!view.hasMarkedText())
        let recoveredEngine = try #require(view.sync.engine)
        #expect(try recoveredEngine.text() == storage.string)

        // A subsequent single-char backspace deletes exactly one character —
        // no drift survived the strand.
        let before = storage.length
        view.setSelectedRange(NSRange(location: before, length: 0))
        view.deleteBackward(nil)
        #expect(storage.length == before - 1)
    }

    /// Common IME strands (Hangul jamo→syllable, accent-popup replacement)
    /// are length-preserving — the recovery must use a content compare, not
    /// a length compare, or this desync would go undetected.
    @Test func lengthPreservingStrandRecoversViaContentCompare() throws {
        let (_, view) = makeHost(text: "alpha bravo")
        let storage = try #require(view.textStorage)
        let originalLength = storage.length
        let originalText = storage.string

        view.setMarkedText(
            "X",
            selectedRange: NSRange(location: 0, length: 1),
            replacementRange: NSRange(location: 0, length: 1)
        )
        #expect(storage.length == originalLength)
        #expect(storage.string != originalText)

        let recovered = view.sync.recoverIfDesynced(
            text: storage.string,
            storage: storage,
            selectedRange: view.selectedRange()
        )

        #expect(recovered)
        let engine = try #require(view.sync.engine)
        #expect(try engine.text() == storage.string)
    }

    @Test func inSyncCallIsANoOp() throws {
        let (_, view) = makeHost(text: "alpha bravo")
        let storage = try #require(view.textStorage)
        let engineBefore = view.sync.engine

        let recovered = view.sync.recoverIfDesynced(
            text: storage.string,
            storage: storage,
            selectedRange: view.selectedRange()
        )

        #expect(recovered == false)
        #expect(view.sync.engine === engineBefore)
    }

    private func makeHost(text: String) -> (NSWindow, DesyncHostTextView) {
        _ = NSApplication.shared
        let view = DesyncHostTextView(usingTextLayoutManager: true)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 300),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        window.contentView = view
        window.makeFirstResponder(view)

        if let storage = view.textStorage {
            view.sync.seed(text: text, storage: storage, selectedRange: NSRange(location: 0, length: 0))
        }
        return (window, view)
    }
}
#endif
