#if canImport(UIKit)
import UIKit

/// Platform font/color aliases so the theme and applier compile unchanged on
/// iOS/Catalyst (UIKit) and native macOS (AppKit).
public typealias KamiFont = UIFont
public typealias KamiColor = UIColor
#elseif canImport(AppKit)
import AppKit

public typealias KamiFont = NSFont
public typealias KamiColor = NSColor
#endif

extension KamiColor {
    /// Primary text color (`.label` / `.labelColor`).
    static var kamiLabel: KamiColor {
        #if canImport(UIKit)
        return .label
        #else
        return .labelColor
        #endif
    }

    /// Dimmed color for revealed markers (`.tertiaryLabel` / `.tertiaryLabelColor`).
    static var kamiTertiaryLabel: KamiColor {
        #if canImport(UIKit)
        return .tertiaryLabel
        #else
        return .tertiaryLabelColor
        #endif
    }
}

extension KamiFont {
    /// Returns a copy with the bold/italic traits added. Trait spellings and
    /// descriptor-init optionality differ between UIKit and AppKit; both are
    /// handled here so callers stay platform-free.
    ///
    /// If the font family has no matching bold/italic face, the original font
    /// is returned unchanged — deliberate graceful degradation (system fonts,
    /// the default theme's only fonts, always have both faces; custom themes
    /// own their font choices and can bypass this helper entirely).
    func kamiAddingTraits(bold: Bool, italic: Bool) -> KamiFont {
        guard bold || italic else { return self }
        #if canImport(UIKit)
        var traits = fontDescriptor.symbolicTraits
        if bold { traits.insert(.traitBold) }
        if italic { traits.insert(.traitItalic) }
        guard let descriptor = fontDescriptor.withSymbolicTraits(traits) else { return self }
        return KamiFont(descriptor: descriptor, size: 0) // size 0 preserves current size
        #else
        var traits = fontDescriptor.symbolicTraits
        if bold { traits.insert(.bold) }
        if italic { traits.insert(.italic) }
        let descriptor = fontDescriptor.withSymbolicTraits(traits)
        return KamiFont(descriptor: descriptor, size: 0) ?? self
        #endif
    }
}
