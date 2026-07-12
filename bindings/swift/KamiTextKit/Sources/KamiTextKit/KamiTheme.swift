#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Maps a segment's composed `Kind` set to visual attributes: kinds are
/// core's semantic classification, styling is the adapter's job.
///
/// Concealed markers use a near-zero-size font to stay effectively
/// invisible while preserving string indices; a small residual glyph
/// advance survives even at that size, so a concealed run's `.font` should
/// be paired with a matching negative `.kern` (e.g. `-pointSize`) —
/// otherwise a long concealed run (`![alt](very/long/url)`) can accumulate
/// a visible sliver. `DefaultKamiTheme` does this at 0.01 pt / `-0.01`.
///
/// Dimmed rendering for a revealed marker is theme styling of `MARKER &&
/// !concealed` — the contract stays two-state (`concealed: Bool`); there is
/// no third "dimmed" state to model here.
///
/// `attributes(for:concealed:)` and `checkedTaskOverlayAttributes()` must
/// return stable values for the lifetime of any applier constructed with
/// this theme; to change theme output, construct a new applier.
public protocol KamiTheme {
    func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any]

    /// Attributes layered ON TOP of the base segment styling for the content
    /// (non-marker) portion of a checked task item. Separate from
    /// `attributes(for:concealed:)` because checked-ness is element state
    /// (`KamiElement.checked`), not a kind bit — the kind-based method never
    /// sees it. Return `[:]` to disable the overlay entirely.
    func checkedTaskOverlayAttributes() -> [NSAttributedString.Key: Any]
}

extension KamiTheme {
    /// Default overlay: strikethrough + dimmed text, matching `DefaultKamiTheme`.
    public func checkedTaskOverlayAttributes() -> [NSAttributedString.Key: Any] {
        [
            .strikethroughStyle: NSUnderlineStyle.single.rawValue,
            .foregroundColor: KamiColor.kamiTertiaryLabel
        ]
    }
}

/// Default theme: system fonts and semantic colors for every kind bit —
/// heading sizes step down from 26pt, code runs get monospaced fonts,
/// `TASK_MARKER` a systemBlue accent when revealed, GFM `STRIKETHROUGH` its
/// own underline styling, and concealed runs the 0.01pt + clear + negative
/// kern hidden treatment.
public struct DefaultKamiTheme: KamiTheme, Sendable {
    private static let headingSizes: [CGFloat] = [30, 25, 22, 20, 19, 18]

    public init() {}

    public func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
        if concealed {
            // Near-zero advance width + clear color; string indices unchanged.
            // `.kern: -0.01` cancels the residual advance the 0.01pt glyph
            // still carries, so long concealed runs (`![alt](very/long/url)`)
            // can't accumulate a visible sliver.
            return [
                .font: KamiFont.systemFont(ofSize: 0.01),
                .foregroundColor: KamiColor.clear,
                .kern: CGFloat(-0.01)
            ]
        }

        let font = baseFont(for: kinds).kamiAddingTraits(
            bold: kinds.contains(.strong),
            italic: kinds.contains(.emphasis)
        )

        var attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: isCode(kinds) ? KamiColor.systemPink : KamiColor.kamiLabel
        ]

        if kinds.contains(.strikethrough) {
            attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }

        if kinds.contains(.taskMarker) {
            attrs[.foregroundColor] = KamiColor.systemBlue
        } else if kinds.contains(.marker) {
            attrs[.foregroundColor] = KamiColor.kamiTertiaryLabel
        }

        return attrs
    }

    private func isCode(_ kinds: KamiKindSet) -> Bool {
        kinds.contains(.codeSpan) || kinds.contains(.codeBlock)
    }

    private func baseFont(for kinds: KamiKindSet) -> KamiFont {
        if isCode(kinds) {
            return .monospacedSystemFont(ofSize: 16, weight: .regular)
        }
        if let level = kinds.headingLevel {
            return .systemFont(ofSize: Self.headingSizes[level - 1], weight: .bold)
        }
        return .systemFont(ofSize: 18)
    }
}
