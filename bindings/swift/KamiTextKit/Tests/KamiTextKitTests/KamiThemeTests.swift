import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// `DefaultKamiTheme` regressions: the conceal kern term, and the
/// heading+strong font-composition case nothing exercised directly before.
@MainActor
struct KamiThemeTests {
    @Test func concealedAttributesCarryNearZeroFontAndKern() {
        let theme = DefaultKamiTheme()
        let attrs = theme.attributes(for: [.emphasis, .marker], concealed: true)

        let font = attrs[.font] as? KamiFont
        #expect(font?.pointSize == 0.01)

        let kern = attrs[.kern] as? CGFloat
        #expect(kern == -0.01)
    }

    /// Locks in "no shrinking-bold-in-heading bug": Kami composes fonts via
    /// analysis.rs's flattened kind bits, so a heading+strong segment must
    /// keep the heading's point size and simply gain the bold trait, not
    /// collapse to the default body size.
    @Test func headingPlusStrongPreservesHeadingSizeAndAddsBoldTrait() throws {
        let theme = DefaultKamiTheme()
        let headingOnly = theme.attributes(for: [.heading2], concealed: false)
        let headingStrong = theme.attributes(for: [.heading2, .strong], concealed: false)

        let headingFont = try #require(headingOnly[.font] as? KamiFont)
        let composedFont = try #require(headingStrong[.font] as? KamiFont)

        #expect(composedFont.pointSize == headingFont.pointSize)
        #if canImport(UIKit)
        #expect(composedFont.fontDescriptor.symbolicTraits.contains(.traitBold))
        #else
        #expect(composedFont.fontDescriptor.symbolicTraits.contains(.bold))
        #endif
    }
}
