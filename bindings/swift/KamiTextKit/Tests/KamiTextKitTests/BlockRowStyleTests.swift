import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Rows that are pure block syntax — fence lines, a setext underline — must lay
/// out like the block they belong to, not as stray body or heading rows. Row
/// height comes from the row's fonts and its paragraph style, and paragraph
/// fixing hands a paragraph its FIRST character's style.
@MainActor
struct BlockRowStyleTests {
    @Test func fenceRowsTakeTheCodeParagraphStyle() throws {
        let text = "x\n```py\nfn\n```\n"   // caret on "x": fence lines concealed
        let storage = try styled(text, theme: CodeLeadingTheme())
        let ns = storage.string as NSString
        let opening = ns.range(of: "```py").location
        let code = ns.range(of: "fn").location
        let closing = ns.range(of: "```", options: .backwards).location
        #expect(lineHeightMultiple(storage, at: code) == CodeLeadingTheme.code)
        #expect(lineHeightMultiple(storage, at: opening) == CodeLeadingTheme.code)
        #expect(lineHeightMultiple(storage, at: closing) == CodeLeadingTheme.code)
        #expect(lineHeightMultiple(storage, at: 0) == CodeLeadingTheme.body)
    }

    @Test func hiddenSetextUnderlineRowHasNoHeadingSizedGlyph() throws {
        let text = "x\n\nTitle\n=====\nbody\n"   // caret on "x": underline concealed
        let storage = try styled(text, theme: DefaultKamiTheme())
        let ns = storage.string as NSString
        let underline = ns.range(of: "=====")
        for index in underline.location...NSMaxRange(underline) {   // run + its "\n"
            let font = storage.attribute(.font, at: index, effectiveRange: nil) as? KamiFont
            #expect((font?.pointSize ?? 0) < 1, "unit \(index) keeps a visible font")
        }
    }

    private func styled(_ text: String, theme: any KamiTheme) throws -> NSTextStorage {
        let engine = try KamiEngine(text: text)
        let storage = NSTextStorage(string: try engine.text())
        try KamiTextStorageApplier(theme: theme).applyFull(engine: engine, to: storage)
        storage.fixAttributes(in: NSRange(location: 0, length: storage.length))
        return storage
    }

    private func lineHeightMultiple(_ storage: NSTextStorage, at location: Int) -> CGFloat {
        (storage.attribute(.paragraphStyle, at: location, effectiveRange: nil) as? NSParagraphStyle)?
            .lineHeightMultiple ?? -1
    }

    /// One paragraph style per paragraph kind keyed off the run's own kind
    /// bits, concealed or not — the shape of the apps' theme adapter.
    private struct CodeLeadingTheme: KamiTheme {
        static let code: CGFloat = 1.4
        static let body: CGFloat = 1.2
        private let codeStyle: NSParagraphStyle = {
            let style = NSMutableParagraphStyle()
            style.lineHeightMultiple = code
            return style.copy() as! NSParagraphStyle
        }()
        private let bodyStyle: NSParagraphStyle = {
            let style = NSMutableParagraphStyle()
            style.lineHeightMultiple = body
            return style.copy() as! NSParagraphStyle
        }()

        func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
            var attrs = DefaultKamiTheme().attributes(for: kinds, concealed: concealed)
            attrs[.paragraphStyle] = kinds.contains(.codeBlock) ? codeStyle : bodyStyle
            return attrs
        }
    }
}
