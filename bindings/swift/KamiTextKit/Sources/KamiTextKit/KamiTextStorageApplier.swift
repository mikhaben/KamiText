#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Applies `KamiEngine` segment/element output onto an `NSTextStorage`.
///
/// Checked-task overlay styling lives here rather than on `KamiTheme` because
/// the engine's `Segment.kinds` bitset has no "this text is inside a checked
/// task" bit — checked/unchecked state lives on `Element`
/// (`KamiElement.checked`, kind `.task`), not on segments. A theme that only
/// ever sees kind bits (per its protocol) can't express that, so this applier
/// queries `elements(in:)` separately and layers the overlay on top.
@MainActor
public struct KamiTextStorageApplier {
    /// The attribute memo is append-only for the applier's lifetime, sound
    /// under `KamiTheme`'s stability requirement (see its doc comment): to
    /// change theme output, construct a new applier.
    public let theme: any KamiTheme

    /// Applier-lifetime cache for `theme.attributes(for:concealed:)` and
    /// `theme.checkedTaskOverlayAttributes()`. A reference type so struct
    /// copies of the applier (sharing the same `theme`) share one cache.
    private let memo = AttributeMemo()

    public init(theme: any KamiTheme = DefaultKamiTheme()) {
        self.theme = theme
    }

    /// Full-document apply: seeds `storage` from scratch. Builds the styled
    /// string detached and swaps it in with ONE `processEditing` pass —
    /// thousands of live `setAttributes` on the storage would each way
    /// trigger attribute fixing, which dominates large seeds (measured ~560ms
    /// vs ~70ms at 250 KB).
    public func applyFull(engine: KamiEngine, to storage: NSTextStorage) throws(KamiEngineError) {
        let range = 0..<engine.lenBytes
        let segments = try engine.segments(in: range)
        let elements = try engine.elements(in: range)
        let built = NSMutableAttributedString(string: try engine.text())
        apply(segments: segments, elements: elements, to: built)
        storage.setAttributedString(built)
    }

    /// Patch-driven apply: re-styles the dirty ranges from a `KamiPatch`
    /// (patches are segment-aligned, so re-fetching just these ranges is
    /// sufficient for segment styling). Dirty ranges that
    /// clip a task element are widened to the whole element first: the
    /// checked-task overlay is adapter state driven by `Element.checked`,
    /// which the core cannot dirty — a checkbox toggle dirties only the
    /// marker bytes, but the content's strikethrough overlay must follow.
    public func apply(_ patch: KamiPatch, engine: KamiEngine, to storage: NSTextStorage) throws(KamiEngineError) {
        for range in patch.dirty {
            var applyRange = range
            // Paragraph widening: the hanging-indent pass keys off a paragraph's
            // LIST MARKER segment, but an edit can dirty only a body run that
            // shares the paragraph (e.g. the nested item's leading whitespace) —
            // the per-segment reset would then wipe the paragraph style at its
            // first character (paragraph fixing propagates it) while the marker
            // never re-enters the pass, losing the indent with no self-heal.
            // Widening every dirty range to whole paragraphs guarantees the
            // marker segment is always present alongside any dirtied byte of
            // its paragraph. (Storage is already post-edit here, so its UTF-16
            // offsets align with the engine's.)
            if let u16Start = try? engine.byteToUtf16(range.lowerBound),
               let u16End = try? engine.byteToUtf16(range.upperBound) {
                let ns = storage.mutableString
                let location = Swift.min(Int(u16Start), ns.length)
                let length = Swift.max(0, Swift.min(Int(u16End), ns.length) - location)
                let para = ns.paragraphRange(for: NSRange(location: location, length: length))
                if let byteStart = try? engine.utf16ToByte(UInt32(para.location)),
                   let byteEnd = try? engine.utf16ToByte(UInt32(NSMaxRange(para))) {
                    applyRange = Swift.min(applyRange.lowerBound, byteStart)
                        ..< Swift.max(applyRange.upperBound, byteEnd)
                }
            }
            var elements = try engine.elements(in: applyRange)
            let beforeTaskWiden = applyRange
            for element in elements where element.kind == .task && intersects(element.range, applyRange) {
                applyRange = Swift.min(applyRange.lowerBound, element.range.lowerBound)
                    ..< Swift.max(applyRange.upperBound, element.range.upperBound)
            }
            if applyRange != beforeTaskWiden {
                elements = try engine.elements(in: applyRange)
            }
            let segments = try engine.segments(in: applyRange)
            apply(segments: segments, elements: elements, to: storage)
        }
    }

    /// Works on any `NSMutableAttributedString`; `NSTextStorage` (a subclass)
    /// gets its `beginEditing`/`endEditing` batching, detached builders are a
    /// cheap no-op pair.
    private func apply(segments: [KamiSegment], elements: [KamiElement], to storage: NSMutableAttributedString) {
        let length = storage.length
        // Theme lookups dominate large applies (font/descriptor churn per
        // call); distinct (kinds, concealed) combos number ~dozens, so the
        // applier-lifetime memo collapses a 15k-segment seed to ~dozens of
        // theme builds total, not per apply.
        storage.beginEditing()
        for segment in segments {
            guard let range = nsRange(segment.utf16Range, clampedTo: length) else { continue }
            // Kind bits occupy 0–47, so << 1 cannot overflow.
            let key = segment.kinds.rawValue << 1 | (segment.concealed ? 1 : 0)
            let attributes: [NSAttributedString.Key: Any]
            if let cached = memo.segment[key] {
                attributes = cached
            } else {
                attributes = theme.attributes(for: segment.kinds, concealed: segment.concealed)
                memo.segment[key] = attributes
            }
            storage.setAttributes(attributes, range: range)
        }
        for element in elements where element.kind == .task && element.checked {
            overlayCheckedTask(element, segments: segments, in: storage, length: length)
        }
        applyListHangingIndents(segments: segments, to: storage, length: length)
        storage.endEditing()
    }

    /// Sets each bullet/ordinal list paragraph's `headIndent` to the rendered
    /// width of its visible prefix (leading whitespace + marker + trailing
    /// space), so soft-wrapped continuation lines hang under the item text.
    /// `firstLineHeadIndent` stays 0 — the marker and any nesting whitespace
    /// are real glyphs on line 1. The value is set ABSOLUTELY, not as a delta:
    /// the per-segment pass just above reset these paragraphs to `listStyle`
    /// (headIndent 0), so this is idempotent and self-healing — a paragraph
    /// that stops being a list is re-styled to `bodyStyle` (headIndent 0) by
    /// that same pass, no sentinel needed. Task paragraphs carry `.taskMarker`
    /// (not `.listBullet`/`.listOrdinal`) and so are never touched; quoted
    /// lists keep `quoteStyle` via the `.blockquote` guard.
    private func applyListHangingIndents(segments: [KamiSegment], to storage: NSMutableAttributedString, length: Int) {
        // Most applies carry no list segments at all — skip before bridging.
        guard segments.contains(where: { $0.kinds.contains(.listBullet) || $0.kinds.contains(.listOrdinal) }) else { return }
        // Live proxy, unlike `string as NSString` which can copy the whole
        // backing store on every apply.
        let ns = storage.mutableString
        for segment in segments {
            guard segment.kinds.contains(.listBullet) || segment.kinds.contains(.listOrdinal),
                  !segment.kinds.contains(.blockquote),
                  let marker = nsRange(segment.utf16Range, clampedTo: length),
                  marker.length > 0 else { continue }
            let para = ns.paragraphRange(for: marker)
            // Advance past the marker's trailing spaces/tabs to the item text.
            var content = NSMaxRange(marker)
            while content < NSMaxRange(para),
                  ns.character(at: content) == 0x20 || ns.character(at: content) == 0x09 {
                content += 1
            }
            let prefixLen = content - para.location
            guard prefixLen > 0 else { continue }
            // Prefix widths repeat massively ("- ", "1. ", "  - "…) and the
            // theme is applier-immutable, so memoize by the prefix STRING —
            // collapsing a list-heavy seed's thousands of CoreText size()
            // calls to a handful. Fonts for a given prefix are deterministic
            // per theme (bullets never conceal; leading whitespace is body).
            let prefixRange = NSRange(location: para.location, length: prefixLen)
            let prefixString = ns.substring(with: prefixRange)
            let width: CGFloat
            if let cached = memo.listPrefixWidth[prefixString] {
                width = cached
            } else {
                width = storage.attributedSubstring(from: prefixRange).size().width
                if memo.listPrefixWidth.count >= 256 { memo.listPrefixWidth.removeAll(keepingCapacity: true) }
                memo.listPrefixWidth[prefixString] = width
            }
            // Base from the PARAGRAPH's first unit (not the marker): a host
            // pass may have inflated fields there (reserve spacing); copying
            // from it preserves them instead of silently reverting the whole
            // paragraph to the theme instance.
            let base = (storage.attribute(.paragraphStyle, at: para.location, effectiveRange: nil)
                as? NSParagraphStyle) ?? NSParagraphStyle()
            guard abs(base.headIndent - width) > 0.5 else { continue } // idempotent no-op
            let mutable = base.mutableCopy() as! NSMutableParagraphStyle
            mutable.headIndent = width // firstLineHeadIndent stays 0
            storage.addAttribute(.paragraphStyle, value: mutable.copy(), range: para)
        }
    }

    /// Layers strikethrough + dimmed color onto the content (non-marker)
    /// portion of a checked task's range — additive on top of the base
    /// per-segment styling, matching the spike's checked-list-item behavior.
    ///
    /// Binary-searches the element's window: segments are sorted, and a
    /// full scan per task element is quadratic on task-heavy documents
    /// (measured as the dominant cost of large seeds).
    private func overlayCheckedTask(_ element: KamiElement, segments: [KamiSegment], in storage: NSMutableAttributedString, length: Int) {
        let overlay: [NSAttributedString.Key: Any]
        if let cached = memo.checkedTaskOverlay {
            overlay = cached
        } else {
            overlay = theme.checkedTaskOverlayAttributes()
            memo.checkedTaskOverlay = overlay
        }
        guard !overlay.isEmpty else { return }
        var low = 0
        var high = segments.count
        while low < high {
            let mid = (low + high) / 2
            if segments[mid].range.upperBound <= element.range.lowerBound {
                low = mid + 1
            } else {
                high = mid
            }
        }
        var index = low
        while index < segments.count, segments[index].range.lowerBound < element.range.upperBound {
            let segment = segments[index]
            index += 1
            guard segment.range.lowerBound >= element.range.lowerBound,
                  segment.range.upperBound <= element.range.upperBound,
                  !segment.kinds.contains(.marker), !segment.kinds.contains(.taskMarker),
                  let range = nsRange(segment.utf16Range, clampedTo: length)
            else { continue }
            storage.addAttributes(overlay, range: range)
        }
    }

    private func nsRange(_ range: Range<UInt32>, clampedTo length: Int) -> NSRange? {
        let nsRange = NSRange(location: Int(range.lowerBound), length: Int(range.upperBound - range.lowerBound))
        guard nsRange.location >= 0, NSMaxRange(nsRange) <= length else { return nil }
        return nsRange
    }
}

/// Applier-lifetime memo for theme attribute lookups. `@MainActor` like the
/// applier, so no locking; struct copies of `KamiTextStorageApplier` share
/// this box, which is sound because they share the same immutable `theme`.
@MainActor
private final class AttributeMemo {
    var segment: [UInt64: [NSAttributedString.Key: Any]] = [:]
    var checkedTaskOverlay: [NSAttributedString.Key: Any]?
    /// List-prefix rendered widths ("- ", "1. ", "  - "…) — see
    /// `applyListHangingIndents`. Bounded; cleared wholesale on overflow.
    var listPrefixWidth: [String: CGFloat] = [:]
}

/// Mirrors core's `ByteRange::intersects` (core/src/types.rs:31-40) exactly:
/// non-empty ranges share a byte; an empty range represents caret
/// containment in the other range.
private func intersects(_ a: Range<UInt32>, _ b: Range<UInt32>) -> Bool {
    if a.isEmpty {
        return b.lowerBound <= a.lowerBound && a.lowerBound < b.upperBound
    } else if b.isEmpty {
        return a.lowerBound <= b.lowerBound && b.lowerBound < a.upperBound
    } else {
        return a.lowerBound < b.upperBound && b.lowerBound < a.upperBound
    }
}
