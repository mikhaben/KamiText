import KamiCore

/// Swift wrapper over the kamitext C ABI.
///
/// Single-threaded (`Engine` is `Send`, not `Sync`); pinning
/// this to the main actor matches that and is where adapters call from.
@MainActor
public final class KamiEngine {
    /// Mirrors `KamiOptions`.
    public struct Options: Sendable {
        public var extensions: Extensions
        public var reveal: RevealMode

        public init(extensions: Extensions = .all, reveal: RevealMode = .line) {
            self.extensions = extensions
            self.reveal = reveal
        }
    }

    /// Mirrors the `KamiOptions.extensions` bitflags.
    public struct Extensions: OptionSet, Sendable {
        public let rawValue: UInt32
        public init(rawValue: UInt32) { self.rawValue = rawValue }

        public static let tables = Extensions(rawValue: 1 << 0)
        public static let taskLists = Extensions(rawValue: 1 << 1)
        public static let strikethrough = Extensions(rawValue: 1 << 2)
        public static let wikilinks = Extensions(rawValue: 1 << 3)
        /// Structural fenced code (host OPT-IN, deliberately NOT in `.all`):
        /// the whole block conceals off-caret behind one Block marker — the
        /// table model — so the host can draw a horizontally scrolling code
        /// view in its place. Only enable with that view implemented, or code
        /// blocks render as blank space.
        public static let structuralCode = Extensions(rawValue: 1 << 4)
        public static let all: Extensions = [.tables, .taskLists, .strikethrough, .wikilinks]
    }

    /// Mirrors `KamiOptions.reveal`.
    public enum RevealMode: UInt32, Sendable {
        case none = 0
        case line = 1
        case block = 2
        case element = 3
    }

    private let handle: OpaquePointer

    public init(text: String, options: Options = Options()) throws(KamiEngineError) {
        let actual = kami_abi_version()
        let expected = UInt32(KAMI_ABI_VERSION)
        guard actual == expected else {
            throw .unsupportedABIVersion(expected: expected, actual: actual)
        }

        let cOptions = KamiCore.KamiOptions(extensions: options.extensions.rawValue, reveal: options.reveal.rawValue)
        let created = Array(text.utf8).withUnsafeBufferPointer { buffer in
            kami_engine_new(buffer.baseAddress, buffer.count, cOptions)
        }
        // No engine handle exists yet to query `kami_last_error_message` from
        // (invalid UTF-8, invalid options, or an internal panic during
        // construction all surface as a bare NULL — an ABI limitation).
        guard let created else {
            throw .constructionFailed
        }
        handle = created
    }

    isolated deinit {
        kami_engine_free(handle)
    }

    // MARK: - Mutation

    public func applyEdit(_ range: Range<UInt32>, replacement: String) throws(KamiEngineError) -> KamiPatch {
        var raw = KamiCore.KamiPatch(ranges: nil, len: 0, generation: 0)
        let code = Array(replacement.utf8).withUnsafeBufferPointer { buffer in
            kami_apply_edit(handle, range.lowerBound, range.upperBound, buffer.baseAddress, buffer.count, &raw)
        }
        try checkError(code)
        return copyPatch(raw)
    }

    public func setSelection(_ range: Range<UInt32>) throws(KamiEngineError) -> KamiPatch {
        var raw = KamiCore.KamiPatch(ranges: nil, len: 0, generation: 0)
        let code = kami_set_selection(handle, range.lowerBound, range.upperBound, &raw)
        try checkError(code)
        return copyPatch(raw)
    }

    // MARK: - Queries

    public func segments(in range: Range<UInt32>) throws(KamiEngineError) -> [KamiSegment] {
        var raw = KamiCore.KamiSegments(ptr: nil, len: 0, generation: 0)
        let code = kami_segments_in(handle, range.lowerBound, range.upperBound, &raw)
        try checkError(code)
        return copySegments(raw)
    }

    public func elements(in range: Range<UInt32>) throws(KamiEngineError) -> [KamiElement] {
        var raw = KamiCore.KamiElements(ptr: nil, len: 0, generation: 0)
        let code = kami_elements_in(handle, range.lowerBound, range.upperBound, &raw)
        try checkError(code)
        return copyElements(raw)
    }

    public func text() throws(KamiEngineError) -> String {
        var raw = KamiCore.KamiStr(ptr: nil, len: 0)
        let code = kami_text(handle, &raw)
        try checkError(code)
        return string(from: raw)
    }

    public var lenBytes: UInt32 {
        kami_len_bytes(handle)
    }

    public var lenUtf16: UInt32 {
        kami_len_utf16(handle)
    }

    // MARK: - Offset conversion

    public func byteToUtf16(_ offset: UInt32) throws(KamiEngineError) -> UInt32 {
        var out: UInt32 = 0
        let code = kami_byte_to_utf16(handle, offset, &out)
        try checkError(code)
        return out
    }

    public func utf16ToByte(_ offset: UInt32) throws(KamiEngineError) -> UInt32 {
        var out: UInt32 = 0
        let code = kami_utf16_to_byte(handle, offset, &out)
        try checkError(code)
        return out
    }

    // MARK: - Typing behaviors

    public func newlinePlan(at offset: UInt32) throws(KamiEngineError) -> KamiEditPlan? {
        var raw = KamiCore.KamiEditPlan(has_plan: 0, _pad: (0, 0, 0), caret: 0, edits: nil, edits_len: 0, generation: 0)
        let code = kami_newline_plan(handle, offset, &raw)
        try checkError(code)
        return copyEditPlan(raw)
    }

    public func toggleTaskPlan(at offset: UInt32) throws(KamiEngineError) -> KamiEditPlan? {
        var raw = KamiCore.KamiEditPlan(has_plan: 0, _pad: (0, 0, 0), caret: 0, edits: nil, edits_len: 0, generation: 0)
        let code = kami_toggle_task_plan(handle, offset, &raw)
        try checkError(code)
        return copyEditPlan(raw)
    }

    // MARK: - Arena copy helpers
    //
    // Every `KamiCore.*` value below points into the engine's arena and is
    // invalidated by the next call on this engine. Each helper
    // copies scalar/collection data into a plain Swift value immediately;
    // none of these ever let a raw pointer escape.

    private func string(from raw: KamiCore.KamiStr) -> String {
        guard let ptr = raw.ptr, raw.len > 0 else { return "" }
        return String(decoding: UnsafeBufferPointer(start: ptr, count: raw.len), as: UTF8.self)
    }

    private func copyPatch(_ raw: KamiCore.KamiPatch) -> KamiPatch {
        guard let ptr = raw.ranges, raw.len > 0 else { return KamiPatch(dirty: []) }
        let dirty = UnsafeBufferPointer(start: ptr, count: raw.len).map { $0.start..<$0.end }
        return KamiPatch(dirty: dirty)
    }

    private func copySegments(_ raw: KamiCore.KamiSegments) -> [KamiSegment] {
        guard let ptr = raw.ptr, raw.len > 0 else { return [] }
        return UnsafeBufferPointer(start: ptr, count: raw.len).map { seg in
            KamiSegment(
                range: seg.start..<seg.end,
                utf16Range: seg.utf16_start..<seg.utf16_end,
                kinds: KamiKindSet(rawValue: seg.kinds),
                concealed: seg.concealed != 0
            )
        }
    }

    private func copyElements(_ raw: KamiCore.KamiElements) -> [KamiElement] {
        guard let ptr = raw.ptr, raw.len > 0 else { return [] }
        return UnsafeBufferPointer(start: ptr, count: raw.len).map { el in
            // The C `checked` byte is overloaded per kind: task checkedness or
            // heading level. Decode by kind so neither leaks into the other.
            let kind = KamiElementKind(rawValue: el.kind)
            return KamiElement(
                id: el.id,
                range: el.start..<el.end,
                kind: kind,
                checked: kind == .task && el.checked != 0,
                level: kind == .heading ? el.checked : 0,
                auxRange: el.aux_start..<el.aux_end
            )
        }
    }

    private func copyEditPlan(_ raw: KamiCore.KamiEditPlan) -> KamiEditPlan? {
        guard raw.has_plan != 0 else { return nil }
        let edits: [KamiEditOp]
        if let ptr = raw.edits, raw.edits_len > 0 {
            edits = UnsafeBufferPointer(start: ptr, count: raw.edits_len).map {
                KamiEditOp(range: $0.start..<$0.end, text: string(from: $0.text))
            }
        } else {
            edits = []
        }
        return KamiEditPlan(caret: raw.caret, edits: edits)
    }

    private func lastErrorMessage() -> String {
        string(from: kami_last_error_message(handle))
    }

    private func checkError(_ code: Int32) throws(KamiEngineError) {
        guard code != KAMI_OK else { return }
        let message = lastErrorMessage()
        switch code {
        case KAMI_ERR_INVALID_RANGE: throw .invalidRange(message: message)
        case KAMI_ERR_INVALID_UTF8: throw .invalidUTF8(message: message)
        case KAMI_ERR_NULL: throw .null(message: message)
        case KAMI_ERR_POISONED: throw .poisoned(message: message)
        default: throw .internalError(message: message)
        }
    }
}
