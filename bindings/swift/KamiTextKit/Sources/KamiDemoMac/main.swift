// Minimal AppKit window proving KamiTextKit on native macOS: an NSTextView
// (TextKit 2) with live hidden-syntax markdown styling driven by KamiTextSync.
// Run: `swift run KamiDemoMac` (window) or `swift run KamiDemoMac --selftest`
// (headless engine/sync checks, exits 0 on success).
#if os(macOS)
import AppKit
import KamiTextKit

let demoDoc = """
# Kami Demo 🎉

Live **bold** and *italic* and `inline code` on native macOS.

## Works everywhere

- [ ] Task with a checkbox
- [x] Completed task
- Plain bullet with ~~strikethrough~~

> Blockquote with **bold** inside.

A paragraph with an emoji 🎨 mid-word like sun🌞shine to prove UTF-16 offset
handling, plus a [link](https://example.com) and a fence:

```swift
let answer = 42
```
"""

/// Demo-local alternate theme: `DefaultKamiTheme` with all visible text
/// forced to one unmistakable color, so a theme toggle is instantly obvious.
/// Concealed runs keep the default hidden treatment untouched.
struct DemoIndigoTheme: KamiTheme {
    private let base = DefaultKamiTheme()

    func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
        var attrs = base.attributes(for: kinds, concealed: concealed)
        if !concealed {
            attrs[.foregroundColor] = NSColor.systemIndigo
        }
        return attrs
    }
}

@MainActor
func runSelftest() -> Int32 {
    let storage = NSTextStorage(string: demoDoc)
    let sync = KamiTextSync()
    sync.seed(text: demoDoc, storage: storage, selectedRange: NSRange(location: 0, length: 0))
    guard let engine = sync.engine else {
        print("SELFTEST FAIL: engine did not construct")
        return 1
    }
    guard engine.lenUtf16 == UInt32(storage.length) else {
        print("SELFTEST FAIL: length desync \(engine.lenUtf16) vs \(storage.length)")
        return 1
    }

    // Simulate a keystroke: insert "X" mid-bold via the willChange/didChange
    // pair, mirroring what the NSTextViewDelegate wiring does.
    let insertAt = (demoDoc as NSString).range(of: "bold").location + 2
    let editRange = NSRange(location: insertAt, length: 0)
    sync.willChange(range: editRange, replacement: "X", storageLength: storage.length, isComposing: false)
    storage.replaceCharacters(in: editRange, with: "X")
    sync.didChange(
        text: storage.string,
        storage: storage,
        selectedRange: NSRange(location: insertAt + 1, length: 0),
        isComposing: false
    )
    guard let engine2 = sync.engine, engine2.lenUtf16 == UInt32(storage.length) else {
        print("SELFTEST FAIL: desync after edit")
        return 1
    }
    guard storage.string.contains("boXld") else {
        print("SELFTEST FAIL: edit not applied")
        return 1
    }

    // Caret on the heading line must reveal its marker (attribute check:
    // the "# " run is clear-colored when concealed, visible when revealed).
    sync.selectionChanged(
        selectedRange: NSRange(location: 2, length: 0),
        text: storage.string, storage: storage, isComposing: false
    )
    let markerColor = storage.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
    guard markerColor != .clear else {
        print("SELFTEST FAIL: heading marker still concealed with caret on its line")
        return 1
    }

    // setTheme host recipe: attribute-only re-theme, then a keystroke must
    // style with the NEW theme (sync and applier stay in agreement).
    let lengthBeforeTheme = storage.length
    do {
        try sync.setTheme(DemoIndigoTheme(), storage: storage)
    } catch {
        print("SELFTEST FAIL: setTheme threw \(error)")
        return 1
    }
    guard storage.length == lengthBeforeTheme else {
        print("SELFTEST FAIL: setTheme changed the text (\(lengthBeforeTheme) -> \(storage.length))")
        return 1
    }
    let bodyIndex = (storage.string as NSString).range(of: "paragraph with").location
    guard storage.attribute(.foregroundColor, at: bodyIndex, effectiveRange: nil) as? NSColor == .systemIndigo else {
        print("SELFTEST FAIL: body text not restyled by setTheme")
        return 1
    }
    let typeAt = bodyIndex + 4
    let typeRange = NSRange(location: typeAt, length: 0)
    sync.willChange(range: typeRange, replacement: "Z", storageLength: storage.length, isComposing: false)
    storage.replaceCharacters(in: typeRange, with: "Z")
    sync.didChange(
        text: storage.string, storage: storage,
        selectedRange: NSRange(location: typeAt + 1, length: 0), isComposing: false
    )
    guard storage.attribute(.foregroundColor, at: typeAt, effectiveRange: nil) as? NSColor == .systemIndigo else {
        print("SELFTEST FAIL: keystroke after setTheme styled with the old theme")
        return 1
    }

    print("SELFTEST PASS: seed + keystroke sync + reveal + re-theme all verified (\(storage.length) utf16 units)")
    return 0
}

/// `NSTextView` subclass wiring the `KamiTextSync` header's two host recipes:
/// stranded-composition recovery on focus regain, and a `typingAttributes`
/// self-heal against the near-zero conceal font.
@MainActor
final class KamiDemoTextView: NSTextView {
    /// Set by `DemoController` right after construction.
    var sync: KamiTextSync?
    /// Cmd+T handler, set by `DemoController` (safe to claim: the font panel
    /// it normally opens is disabled with `isRichText = false`).
    var onToggleTheme: (() -> Void)?

    /// Substituted for any `typingAttributes` font under 1pt — matches
    /// `DefaultKamiTheme`'s body-text size.
    private static let bodyFont = NSFont.systemFont(ofSize: 18)

    // AppKit recovery recipe: a view cannot become first responder while it
    // holds an active composition, so focus regain reliably signals any
    // stranded marked text is over, even if a normal `didChange` never comes.
    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became, let sync, let storage = textStorage {
            if hasMarkedText() { unmarkText() }
            _ = sync.recoverIfDesynced(text: storage.string, storage: storage, selectedRange: selectedRange())
        }
        return became
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "t",
           let onToggleTheme {
            onToggleTheme()
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    // typingAttributes self-heal: NSTextView copies whatever attributes sit
    // beside the caret into new typing attributes, including the near-zero
    // conceal font — refuse it so typing/composing beside a hidden
    // delimiter always renders visibly.
    override var typingAttributes: [NSAttributedString.Key: Any] {
        get { super.typingAttributes }
        set {
            var attrs = newValue
            if let font = attrs[.font] as? NSFont, font.pointSize < 1 {
                attrs[.font] = Self.bodyFont
            }
            super.typingAttributes = attrs
        }
    }
}

@MainActor
final class DemoController: NSObject, NSApplicationDelegate, NSTextViewDelegate {
    private var window: NSWindow?
    private var textView: KamiDemoTextView?
    private let sync: KamiTextSync
    private var isAlternateTheme = false

    init(reveal: KamiEngine.RevealMode) {
        sync = KamiTextSync(options: .init(reveal: reveal))
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let contentRect = NSRect(x: 0, y: 0, width: 720, height: 640)
        let textView = KamiDemoTextView(usingTextLayoutManager: true)
        textView.sync = sync
        textView.onToggleTheme = { [weak self] in self?.toggleTheme() }
        textView.frame = contentRect
        textView.autoresizingMask = [.width]
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.minSize = NSSize(width: 0, height: 0)
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.textContainer?.widthTracksTextView = true
        textView.textContainerInset = NSSize(width: 20, height: 24)
        textView.allowsUndo = true
        // Markdown source editing: rich-text commands (Cmd+B, font panel)
        // and smart substitutions would corrupt syntax or fight the engine.
        textView.isRichText = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.delegate = self

        let scroll = NSScrollView(frame: contentRect)
        scroll.hasVerticalScroller = true
        scroll.documentView = textView

        let window = NSWindow(
            contentRect: contentRect,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Kami Demo — native macOS"
        window.contentView = scroll
        window.center()
        window.makeKeyAndOrderFront(nil)

        textView.string = demoDoc
        if let storage = textView.textStorage {
            sync.seed(text: demoDoc, storage: storage, selectedRange: textView.selectedRange())
        }

        self.window = window
        self.textView = textView
        NSApp.activate()
        print("KamiDemoMac: window up, TextKit2=\(textView.textLayoutManager != nil), engine=\(sync.engine != nil)")
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    // MARK: - Re-theming (Cmd+T) — the `KamiTextSync.setTheme` host recipe

    /// Both host moves from the `setTheme` doc: preserve the selection around
    /// the call, and refresh `typingAttributes` afterwards (the view copied
    /// them from the pre-swap caret neighborhood — without this, the next
    /// typed character renders with the OLD theme until the caret moves).
    private func toggleTheme() {
        guard let textView, let storage = textView.textStorage else { return }
        isAlternateTheme.toggle()
        let theme: any KamiTheme = isAlternateTheme ? DemoIndigoTheme() : DefaultKamiTheme()
        let selection = textView.selectedRange()
        do {
            try sync.setTheme(theme, storage: storage)
        } catch {
            print("KamiDemoMac: setTheme failed: \(error)")
            isAlternateTheme.toggle()
            return
        }
        textView.setSelectedRange(selection)
        textView.typingAttributes = theme.attributes(for: [], concealed: false)
        print("KamiDemoMac: theme -> \(isAlternateTheme ? "DemoIndigoTheme" : "DefaultKamiTheme")")
    }

    // MARK: - NSTextViewDelegate → KamiTextSync

    // Plural variant: when implemented, AppKit calls ONLY this one, covering
    // multi-cursor / Replace All edits the singular hook would misreport.
    // Single-range edits stash normally; multi-range edits don't stash, so
    // `didChange` falls through to KamiTextSync's reseed fallback. A nil
    // `replacementStrings` means attribute-only change; no engine edit needed.
    func textView(
        _ textView: NSTextView,
        shouldChangeTextInRanges affectedRanges: [NSValue],
        replacementStrings: [String]?
    ) -> Bool {
        if let replacementStrings,
           affectedRanges.count == 1,
           let range = affectedRanges.first?.rangeValue,
           let replacement = replacementStrings.first,
           let storage = textView.textStorage {
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
        guard let textView, let storage = textView.textStorage else { return }
        sync.didChange(
            text: storage.string,
            storage: storage,
            selectedRange: textView.selectedRange(),
            isComposing: textView.hasMarkedText()
        )
    }

    func textViewDidChangeSelection(_ notification: Notification) {
        guard let textView, let storage = textView.textStorage else { return }
        sync.selectionChanged(
            selectedRange: textView.selectedRange(),
            text: storage.string,
            storage: storage,
            isComposing: textView.hasMarkedText()
        )
    }
}

if CommandLine.arguments.contains("--selftest") {
    exit(runSelftest())
}

/// `--reveal element|line|none` launch argument; defaults to `.line`.
func parseRevealArgument() -> KamiEngine.RevealMode {
    guard let flagIndex = CommandLine.arguments.firstIndex(of: "--reveal"),
          CommandLine.arguments.indices.contains(flagIndex + 1) else {
        return .line
    }
    switch CommandLine.arguments[flagIndex + 1] {
    case "element": return .element
    case "none": return .none
    default: return .line
    }
}

let app = NSApplication.shared
let controller = DemoController(reveal: parseRevealArgument())
app.delegate = controller
app.setActivationPolicy(.regular)
app.run()
#endif
