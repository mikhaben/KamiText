#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Reusable engine↔text-view synchronizer: translates platform
/// edit/selection events into engine calls and applies returned patches to an
/// `NSTextStorage`. Platform-free by construction — it only speaks
/// `NSTextStorage`, `NSRange` (UTF-16) and `String`, which UIKit and AppKit
/// share — so one implementation serves `UITextView` and `NSTextView` hosts.
///
/// Host wiring (three delegate calls + one seed):
/// - UIKit: `shouldChangeTextIn` → `willChange`, `textViewDidChange` →
///   `didChange`, `textViewDidChangeSelection` → `selectionChanged`;
///   `isComposing` = `markedTextRange != nil`.
/// - AppKit: `shouldChangeTextIn:replacementString:` → `willChange`,
///   `textDidChange` → `didChange`, `textViewDidChangeSelection` →
///   `selectionChanged`; `isComposing` = `hasMarkedText()`.
///
/// The host owns caret/scroll preservation around these calls (platform
/// specifics stay out of this type — adapters own them).
///
/// Host caveats (AppKit, empirically verified):
/// - **Undo/redo** fires neither edit hook — only a selection change. A
///   length-changing undo is recovered here (see `selectionChanged`); a
///   length-preserving one heals on the next length change. For bulletproof
///   coverage, hosts add an `NSTextStorageDelegate.didProcessEditing` hook.
/// - **Multi-range edits** (multi-cursor, Replace All): implement the plural
///   `shouldChangeTextInRanges` delegate and call `willChange` only for
///   single-range edits — multi-range didChange then reseeds via fallback.
///
/// Stranded-composition recovery: macOS inline predictions and
/// similar can set marked text outside CJK/accent input, and an interrupted
/// composition can leave `hasMarkedText()`/`markedTextRange` stuck. This type
/// — correctly refusing to sync mid-composition — never resyncs unless a
/// normal `didChange(isComposing: false)` eventually arrives, which a
/// stranded composition may never produce. A view cannot become first
/// responder while it holds an active composition, so focus regain is a
/// reliable out-of-band "composition is over" signal. Host recipes:
/// - **AppKit**: override `becomeFirstResponder()` on the `NSTextView`
///   subclass; when `super.becomeFirstResponder()` returns `true`, call
///   `unmarkText()` if `hasMarkedText()`, then `recoverIfDesynced(text:
///   storage:selectedRange:)`.
/// - **UIKit**: the same shape on the
///   `UITextView` subclass — override `becomeFirstResponder()`; the
///   composition signal is `markedTextRange != nil` (UIKit's equivalent of
///   `hasMarkedText()`); `unmarkText()` is shared API; then
///   `recoverIfDesynced`.
/// - **typingAttributes self-heal** (both platforms): hosts that own the
///   text view should refuse a `< 1 pt` font in `typingAttributes` (the
///   text view copies whatever attributes sit beside the caret into new
///   typing attributes, including the near-zero conceal font) and
///   substitute the body font instead — otherwise typing or composing right
///   after a hidden delimiter silently inherits an invisible font.
@MainActor
public final class KamiTextSync {
    public private(set) var engine: KamiEngine?
    private var applier: KamiTextStorageApplier
    private let options: KamiEngine.Options

    /// Set by `willChange` (pre-edit engine state, byte offsets already
    /// converted; `utf16Location` kept for post-edit verification) and
    /// consumed by `didChange`.
    private var pendingEdit: (byteStart: UInt32, byteEnd: UInt32, replacement: String, utf16Location: Int)?

    /// Checked-task byte ranges as of the last successful sync (seed, or a
    /// `didChange` edit-apply). `KamiTextStorageApplier.apply`'s dirty-range
    /// widening (see its doc comment, `KamiTextStorageApplier.swift`) only
    /// ever sees POST-edit `elements(in:)` — a task an edit destroys or
    /// shrinks has no post-edit element there, so its former content outside
    /// the engine's own dirty range never gets re-styled and keeps a stale
    /// checked-task overlay. This snapshot lets `didChange` diff pre- and
    /// post-edit checked ranges and repaint whatever the edit invalidated.
    /// Reseeding (`seed`) resets it from scratch; selection-only changes
    /// never touch it.
    private var checkedTaskByteRanges: [Range<UInt32>] = []

    public init(theme: any KamiTheme = DefaultKamiTheme(), options: KamiEngine.Options = .init()) {
        self.applier = KamiTextStorageApplier(theme: theme)
        self.options = options
    }

    // MARK: - Seeding

    /// Construct + full apply + selection sync, in one recipe. Call for the
    /// initial load and after any programmatic text assignment (platforms do
    /// NOT fire edit delegates for those), and as the desync/composition-end
    /// fallback. One full parse — none of these call sites are on the hot
    /// per-keystroke path.
    public func seed(text: String, storage: NSTextStorage, selectedRange: NSRange) {
        pendingEdit = nil
        do {
            let engine = try KamiEngine(text: text, options: options)
            self.engine = engine
            try applier.applyFull(engine: engine, to: storage)
            checkedTaskByteRanges = try checkedTaskRanges(engine: engine)
        } catch {
            log("seed error: \(error)")
            engine = nil
            checkedTaskByteRanges = []
            return
        }
        selectionChanged(selectedRange: selectedRange, text: text, storage: storage, isComposing: false)
        assertSynced(storageLength: storage.length)
    }

    // MARK: - Re-theming

    /// Swaps the theme and re-styles the whole document as an attribute-only
    /// pass: no character edits, so nothing lands on the undo stack and an
    /// `NSTextStorageDelegate` sees `.editedAttributes` only — dirty-tracking
    /// hosts (which key off `.editedCharacters`) are unaffected. Subsequent
    /// keystroke patches style with the new theme (sync and applier stay in
    /// agreement). Selection and caret are untouched.
    ///
    /// Runs live per-segment restyling (not the detached `applyFull` swap,
    /// which WOULD register as a character edit); on very large documents
    /// this costs a full-attribute pass — acceptable for an explicit,
    /// user-initiated theme or type-settings change. Hosts should wrap the
    /// call in their caret/scroll preservation like any other sync call, and
    /// refresh the view's `typingAttributes` afterwards — the view copied
    /// them from the pre-swap caret neighborhood, so the next typed
    /// character would otherwise render with the OLD theme until the caret
    /// moves (`KamiDemoMac` demonstrates both host moves).
    public func setTheme(_ theme: any KamiTheme, storage: NSTextStorage) throws(KamiEngineError) {
        applier = KamiTextStorageApplier(theme: theme)
        guard let engine, engine.lenBytes > 0 else { return }
        try applier.apply(KamiPatch(dirty: [0..<engine.lenBytes]), engine: engine, to: storage)
    }

    // MARK: - Edit capture

    /// Pre-edit hook: the engine still holds the pre-edit document here,
    /// which is why the utf16→byte conversion must happen now, not in
    /// `didChange`. Never stashes while IME composition is active — the
    /// reseed path handles composition end instead. A pre-edit length
    /// mismatch means the engine was already desynced: don't stash, so
    /// `didChange` falls through to its reseed fallback.
    public func willChange(range: NSRange, replacement: String, storageLength: Int, isComposing: Bool) {
        guard !isComposing, let engine, engine.lenUtf16 == UInt32(storageLength),
              let utf16Start = UInt32(exactly: range.location),
              let utf16End = UInt32(exactly: range.location + range.length) else {
            // Includes pathological ranges (NSNotFound) — no stash, so
            // `didChange` falls through to its reseed fallback.
            pendingEdit = nil
            return
        }
        do {
            let byteStart = try engine.utf16ToByte(utf16Start)
            let byteEnd = try engine.utf16ToByte(utf16End)
            pendingEdit = (byteStart, byteEnd, replacement, range.location)
        } catch {
            log("willChange conversion error: \(error)")
            pendingEdit = nil
        }
    }

    /// Post-edit hook: applies the stashed edit, or falls back to a full
    /// reseed when nothing was stashed (programmatic mutation that skipped
    /// `willChange`, or IME composition just ended). While composition is
    /// still active, does nothing — engine sync must never happen
    /// mid-composition.
    public func didChange(text: String, storage: NSTextStorage, selectedRange: NSRange, isComposing: Bool) {
        guard !isComposing else {
            // A stash from before composition started is stale the moment
            // marked text lands; drop it so composition end reseeds cleanly.
            pendingEdit = nil
            return
        }

        // `engine.lenUtf16` is necessarily still the PRE-edit length here
        // (the platform already mutated `storage`, the engine hasn't been
        // told yet) — that mismatch is what `applyEdit` resolves, not a
        // desync. The desync check already happened in `willChange`.
        guard let pending = pendingEdit, let engine else {
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return
        }
        // Consume the stash NOW (not via defer, which runs only at return):
        // the trailing `selectionChanged` below guards on `pendingEdit == nil`
        // and must see it cleared to actually re-sync the selection.
        pendingEdit = nil

        // Verify the stash describes the mutation that actually happened:
        // the platform may have suppressed or altered the announced edit
        // (another delegate veto, autocorrect rewrite). The replaced region
        // of the post-edit text must equal the stashed replacement — if not,
        // applying the stash would silently desync the engine.
        let nsText = text as NSString
        let replacedRange = NSRange(location: pending.utf16Location, length: (pending.replacement as NSString).length)
        guard NSMaxRange(replacedRange) <= nsText.length,
              nsText.substring(with: replacedRange) == pending.replacement else {
            log("pending edit does not match storage; reseeding")
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return
        }

        do {
            let patch = try engine.applyEdit(pending.byteStart..<pending.byteEnd, replacement: pending.replacement)
            try applier.apply(patch, engine: engine, to: storage)
        } catch {
            log("applyEdit error: \(error); reseeding")
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return
        }

        guard engine.lenUtf16 == UInt32(storage.length) else {
            log("desync after applyEdit (engine=\(engine.lenUtf16) storage=\(storage.length)); reseeding")
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return
        }

        do {
            try reconcileCheckedTaskOverlays(
                editByteStart: pending.byteStart, editByteEnd: pending.byteEnd,
                replacement: pending.replacement, engine: engine, storage: storage
            )
        } catch {
            log("checked-task reconciliation error: \(error); reseeding")
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return
        }

        assertSynced(storageLength: storage.length)
        selectionChanged(selectedRange: selectedRange, text: text, storage: storage, isComposing: false)
    }

    // MARK: - Checked-task overlay reconciliation

    /// Diffs `checkedTaskByteRanges` (pre-edit) against a fresh post-edit
    /// checked-task query, repaints whatever a destroyed or shrunk task left
    /// behind — see `checkedTaskByteRanges`'s doc comment and
    /// `KamiTextStorageApplier.apply`'s (`KamiTextStorageApplier.swift`) for
    /// why this adapter-side tracking exists — then stores the fresh list.
    private func reconcileCheckedTaskOverlays(
        editByteStart: UInt32, editByteEnd: UInt32, replacement: String,
        engine: KamiEngine, storage: NSTextStorage
    ) throws(KamiEngineError) {
        let replacementByteLen = UInt32(replacement.utf8.count)
        let totalBytes = engine.lenBytes
        let freshChecked = try checkedTaskRanges(engine: engine)

        for previous in checkedTaskByteRanges {
            let shifted = shiftedByteRange(
                previous,
                editByteStart: editByteStart, editByteEnd: editByteEnd,
                replacementByteLen: replacementByteLen, totalBytes: totalBytes
            )
            guard !shifted.isEmpty else { continue }
            let stillCovered = freshChecked.contains {
                $0.lowerBound <= shifted.lowerBound && $0.upperBound >= shifted.upperBound
            }
            guard !stillCovered else { continue }
            try applier.apply(KamiPatch(dirty: [shifted]), engine: engine, to: storage)
        }

        checkedTaskByteRanges = freshChecked
    }

    /// Current checked-task element ranges, queried fresh from the engine.
    private func checkedTaskRanges(engine: KamiEngine) throws(KamiEngineError) -> [Range<UInt32>] {
        try engine.elements(in: 0..<engine.lenBytes)
            .filter { $0.kind == .task && $0.checked }
            .map(\.range)
    }

    /// Shifts a pre-edit byte range by a single edit's byte delta. A range
    /// entirely before the edit is untouched; entirely at/after it moves by
    /// `delta`; one the edit intersects takes a conservative union so a
    /// marker-breaking or checkbox-deleting edit can't undershoot the
    /// content that needs repainting. Clamped to `0...totalBytes` (the
    /// post-edit document length) so a range that lost its tail to a
    /// deletion doesn't point past the end of the document.
    private func shiftedByteRange(
        _ range: Range<UInt32>,
        editByteStart: UInt32, editByteEnd: UInt32, replacementByteLen: UInt32, totalBytes: UInt32
    ) -> Range<UInt32> {
        let delta = Int(replacementByteLen) - Int(editByteEnd - editByteStart)
        let start: Int
        let end: Int
        if range.upperBound <= editByteStart {
            start = Int(range.lowerBound)
            end = Int(range.upperBound)
        } else if range.lowerBound >= editByteEnd {
            start = Int(range.lowerBound) + delta
            end = Int(range.upperBound) + delta
        } else {
            start = Int(min(range.lowerBound, editByteStart))
            end = range.upperBound <= editByteEnd ? Int(editByteStart) + Int(replacementByteLen) : Int(range.upperBound) + delta
        }
        let clampedStart = min(max(start, 0), Int(totalBytes))
        let clampedEnd = min(max(end, clampedStart), Int(totalBytes))
        return UInt32(clampedStart)..<UInt32(clampedEnd)
    }

    // MARK: - Selection

    /// Selection hook. Safe to call unconditionally: it skips while an edit
    /// is in flight (platforms fire selection changes BEFORE the post-edit
    /// hook during a keystroke) or while composing; `didChange` re-syncs the
    /// selection itself once the edit lands.
    public func selectionChanged(selectedRange: NSRange, text: String, storage: NSTextStorage, isComposing: Bool) {
        guard pendingEdit == nil, !isComposing, let engine else { return }
        guard engine.lenUtf16 == UInt32(storage.length) else {
            // Undo/redo bypasses both edit hooks on AppKit (verified: Cmd+Z
            // fires only a selection change) — a length mismatch here with no
            // edit in flight IS the §4.6 desync, so recover instead of going
            // silent. A length-preserving undo still slips past this guard
            // until the next length change; hosts wanting bulletproof undo
            // coverage add an `NSTextStorageDelegate.didProcessEditing` hook
            // and reseed from there.
            log("length desync with no edit in flight (undo/redo?); reseeding")
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return
        }
        do {
            let byteStart = try engine.utf16ToByte(UInt32(selectedRange.location))
            let byteEnd = try engine.utf16ToByte(UInt32(selectedRange.location + selectedRange.length))
            let patch = try engine.setSelection(byteStart..<byteEnd)
            try applier.apply(patch, engine: engine, to: storage)
        } catch {
            log("selection sync error: \(error); reseeding")
            seed(text: text, storage: storage, selectedRange: selectedRange)
        }
    }

    // MARK: - Stranded-composition recovery

    /// Recovery hook for the host's focus-regain recipe (type doc above): a
    /// view cannot become first responder while it holds an active
    /// composition, so regaining focus reliably signals that any stranded
    /// marked-text composition is over, even when the platform's normal
    /// `didChange(isComposing: false)` never arrives to trigger the existing
    /// reseed fallback.
    ///
    /// Desync check is a full content compare (`engine.text() != text`), not
    /// a length compare: the common IME strands (Hangul jamo→syllable,
    /// accent-popup replacement) are length-preserving, so a length-only
    /// check would miss a sticky desync. Falls back to the length check only
    /// if `engine.text()` itself throws.
    ///
    /// Reseeds via the existing `seed` path and returns `true` when desynced
    /// (or when no engine exists yet); returns `false` with no side effects
    /// when already in sync.
    public func recoverIfDesynced(text: String, storage: NSTextStorage, selectedRange: NSRange) -> Bool {
        guard let engine else {
            seed(text: text, storage: storage, selectedRange: selectedRange)
            return true
        }
        let desynced: Bool
        do {
            desynced = try engine.text() != text
        } catch {
            desynced = engine.lenUtf16 != UInt32(storage.length)
        }
        guard desynced else { return false }
        seed(text: text, storage: storage, selectedRange: selectedRange)
        return true
    }

    // MARK: - Desync guard

    private func assertSynced(storageLength: Int) {
        guard let engine else { return }
        assert(engine.lenUtf16 == UInt32(storageLength), "engine/storage length desync")
    }

    private func log(_ message: String) {
        print("KamiTextSync: \(message)")
    }
}
