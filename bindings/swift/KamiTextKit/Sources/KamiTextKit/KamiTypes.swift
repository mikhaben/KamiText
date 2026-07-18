/// Swift-side mirrors of the kamitext C ABI value types.
///
/// These are plain value types copied out of the engine's arena-owned C
/// structs — see `KamiEngine`'s `copy*` helpers, which are the only place
/// that ever touches the raw `KamiCore` pointers.

/// Composed style-kind bitset for a segment.
///
/// Bits 0-22 are the stable v0 kinds; bits 48-63 are reserved/experimental
/// and must never crash — an unrecognized bit simply isn't matched by any
/// `contains(_:)` check a theme performs, which is exactly the intended
/// "degrade to BODY" behavior.
public struct KamiKindSet: OptionSet, Sendable, Hashable {
    public let rawValue: UInt64

    public init(rawValue: UInt64) {
        self.rawValue = rawValue
    }

    public static let body = KamiKindSet(rawValue: 1 << 0)
    public static let heading1 = KamiKindSet(rawValue: 1 << 1)
    public static let heading2 = KamiKindSet(rawValue: 1 << 2)
    public static let heading3 = KamiKindSet(rawValue: 1 << 3)
    public static let heading4 = KamiKindSet(rawValue: 1 << 4)
    public static let heading5 = KamiKindSet(rawValue: 1 << 5)
    public static let heading6 = KamiKindSet(rawValue: 1 << 6)
    public static let strong = KamiKindSet(rawValue: 1 << 7)
    public static let emphasis = KamiKindSet(rawValue: 1 << 8)
    public static let strikethrough = KamiKindSet(rawValue: 1 << 9)
    public static let codeSpan = KamiKindSet(rawValue: 1 << 10)
    public static let codeBlock = KamiKindSet(rawValue: 1 << 11)
    public static let fenceInfo = KamiKindSet(rawValue: 1 << 12)
    public static let blockquote = KamiKindSet(rawValue: 1 << 13)
    public static let listBullet = KamiKindSet(rawValue: 1 << 14)
    public static let listOrdinal = KamiKindSet(rawValue: 1 << 15)
    public static let taskMarker = KamiKindSet(rawValue: 1 << 16)
    public static let link = KamiKindSet(rawValue: 1 << 17)
    public static let image = KamiKindSet(rawValue: 1 << 18)
    public static let table = KamiKindSet(rawValue: 1 << 19)
    public static let thematicBreak = KamiKindSet(rawValue: 1 << 20)
    public static let marker = KamiKindSet(rawValue: 1 << 21)
    public static let htmlRaw = KamiKindSet(rawValue: 1 << 22)

    /// The heading bit present in this set, if any (1-6), else `nil` for body text.
    public var headingLevel: Int? {
        let headings: [(KamiKindSet, Int)] = [
            (.heading1, 1), (.heading2, 2), (.heading3, 3),
            (.heading4, 4), (.heading5, 5), (.heading6, 6)
        ]
        return headings.first { contains($0.0) }?.1
    }
}

/// A maximal run of text with a constant style-kind set and conceal state.
/// Byte and UTF-16 ranges are supplied by the engine — adapters never
/// compute them.
public struct KamiSegment: Sendable, Equatable {
    public let range: Range<UInt32>
    public let utf16Range: Range<UInt32>
    public let kinds: KamiKindSet
    public let concealed: Bool
}

/// `KamiElement.kind` tag (mirrors `KAMI_ELEMENT_*`).
public enum KamiElementKind: UInt32, Sendable, Equatable {
    case task = 0
    case link = 1
    case image = 2
    case fence = 3
    case wikilink = 4
    case heading = 5
}

/// A semantic object an adapter may want to make interactive: task checkbox,
/// link, image, fence, wikilink, or heading (whose `auxRange` is the source
/// title range and `level` carries 1–6).
///
/// `kind` is `nil` when the C ABI's tag isn't one of the known cases —
/// adapters must ignore elements they don't recognize rather than guess, so
/// callers should treat `nil` as "not interactive."
public struct KamiElement: Sendable, Equatable {
    public let id: UInt32
    public let range: Range<UInt32>
    public let kind: KamiElementKind?
    public let checked: Bool
    /// Heading level 1–6 (the C 'checked' byte); 0 for non-headings.
    public let level: UInt8
    public let auxRange: Range<UInt32>
}

/// The set of byte ranges whose segments changed after a mutating call.
/// The adapter's only re-style obligation.
public struct KamiPatch: Sendable, Equatable {
    public let dirty: [Range<UInt32>]
}

/// A single suggested text mutation within an `KamiEditPlan`.
public struct KamiEditOp: Sendable, Equatable {
    public let range: Range<UInt32>
    public let text: String
}

/// A suggested text mutation for a typing behavior. `nil`
/// return from `newlinePlan`/`toggleTaskPlan` means "no plan" (e.g. insert a
/// plain newline yourself), not an error.
public struct KamiEditPlan: Sendable, Equatable {
    public let caret: UInt32
    public let edits: [KamiEditOp]
}

/// Errors surfaced by `KamiEngine`, mapped from the C ABI's `i32` error codes
/// plus two adapter-side conditions the ABI can't attach a diagnostic
/// message to (no engine handle exists yet in either case).
public enum KamiEngineError: Error, Sendable, Equatable {
    case invalidRange(message: String)
    case invalidUTF8(message: String)
    case null(message: String)
    case poisoned(message: String)
    case internalError(message: String)
    case unsupportedABIVersion(expected: UInt32, actual: UInt32)
    case constructionFailed
}
