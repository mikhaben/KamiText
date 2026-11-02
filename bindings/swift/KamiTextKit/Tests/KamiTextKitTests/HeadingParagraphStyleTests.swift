import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Text systems give a paragraph its FIRST character's paragraph style
/// (`fixAttributes` paragraph fixing, which TextKit runs before layout). An ATX
/// heading's first character is its `#` marker, so a theme that keys heading
/// spacing off the heading kind (as the apps' theme does) must see that kind on
/// the marker run — concealed or revealed — or the heading lays out with body
/// spacing.
@MainActor
struct HeadingParagraphStyleTests {
    @Test(arguments: [
        // Caret on "body": the heading's `# ` is concealed.
        ("body\n# Title\nmore\n", 0),
        // Caret on the heading line: the `# ` is revealed.
        ("body\n# Title\nmore\n", 7),
        // Closing sequence present, caret elsewhere.
        ("body\n## Title ##\nmore\n", 0),
    ])
    func atxHeadingParagraphKeepsHeadingStyle(text: String, caret: UInt32) throws {
        let engine = try KamiEngine(text: text)
        _ = try engine.setSelection(caret..<caret)
        let storage = NSTextStorage(string: try engine.text())
        try KamiTextStorageApplier(theme: HeadingSpacingTheme()).applyFull(engine: engine, to: storage)
        storage.fixAttributes(in: NSRange(location: 0, length: storage.length))

        let ns = storage.string as NSString
        let title = ns.range(of: "Title").location
        let more = ns.range(of: "more").location
        #expect(spacingBefore(storage, at: title) == HeadingSpacingTheme.headingSpacingBefore)
        #expect(spacingBefore(storage, at: more) == 0)
    }

    private func spacingBefore(_ storage: NSTextStorage, at location: Int) -> CGFloat {
        (storage.attribute(.paragraphStyle, at: location, effectiveRange: nil) as? NSParagraphStyle)?
            .paragraphSpacingBefore ?? -1
    }

    /// `DefaultKamiTheme` plus one paragraph style per paragraph kind, selected
    /// by the segment's own kind bits for concealed and revealed runs alike —
    /// the shape of the apps' theme adapter.
    private struct HeadingSpacingTheme: KamiTheme {
        static let headingSpacingBefore: CGFloat = 24
        private let heading: NSParagraphStyle = {
            let style = NSMutableParagraphStyle()
            style.paragraphSpacingBefore = headingSpacingBefore
            return style.copy() as! NSParagraphStyle
        }()
        private let body = NSParagraphStyle()

        func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
            var attrs = DefaultKamiTheme().attributes(for: kinds, concealed: concealed)
            attrs[.paragraphStyle] = kinds.headingLevel != nil ? heading : body
            return attrs
        }
    }
}
