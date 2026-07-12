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
            var elements = try engine.elements(in: range)
            for element in elements where element.kind == .task && intersects(element.range, range) {
                applyRange = Swift.min(applyRange.lowerBound, element.range.lowerBound)
                    ..< Swift.max(applyRange.upperBound, element.range.upperBound)
            }
            if applyRange != range {
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
        storage.endEditing()
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
