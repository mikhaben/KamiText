@testable import KamiTextKit

/// Decodable mirror of the `fixtures/*.json` schema. Only the
/// fields the conformance replay asserts are modeled; unused JSON keys
/// (`schema`, `expect.patches`) are simply ignored by `JSONDecoder`.
struct Fixture: Decodable {
    let name: String
    let options: FixtureOptions
    let text: String
    let ops: [FixtureOp]
    let expect: FixtureExpect
}

struct FixtureOptions: Decodable {
    let extensions: [String]
    let reveal: String
}

enum FixtureOp: Decodable {
    case edit(start: UInt32, end: UInt32, insert: String)
    case selection(start: UInt32, end: UInt32)

    private enum CodingKeys: String, CodingKey {
        case type, start, end, insert
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        let start = try container.decode(UInt32.self, forKey: .start)
        let end = try container.decode(UInt32.self, forKey: .end)
        switch type {
        case "edit":
            let insert = try container.decode(String.self, forKey: .insert)
            self = .edit(start: start, end: end, insert: insert)
        case "selection":
            self = .selection(start: start, end: end)
        default:
            throw DecodingError.dataCorruptedError(forKey: .type, in: container, debugDescription: "unknown op type \(type)")
        }
    }
}

struct FixtureExpect: Decodable {
    let segments: [FixtureSegment]
    let elements: [FixtureElement]
    let text: String
    let lenBytes: UInt32
    let lenUtf16: UInt32
}

/// One `expect.elements` entry. The JSON is a discriminated union whose
/// payload key name differs per `kind`, so the payload is an enum decoded by
/// switching on that string: a flat all-optional struct would accept an
/// `image` carrying a `dest` without complaint.
struct FixtureElement {
    let id: UInt32
    let range: FixtureRange
    let payload: Payload

    enum Payload {
        case task(checked: Bool)
        case link(dest: FixtureRange)
        case image(src: FixtureRange, wiki: Bool)
        case fence(info: FixtureRange)
        case wikilink(target: FixtureRange)
        case heading(level: UInt8, text: FixtureRange)
    }
}

extension FixtureElement: Decodable {
    private enum CodingKeys: String, CodingKey {
        case id, range, kind, checked, dest, src, wiki, info, target, level, text
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UInt32.self, forKey: .id)
        range = try container.decode(FixtureRange.self, forKey: .range)
        let kind = try container.decode(String.self, forKey: .kind)
        switch kind {
        case "task":
            payload = .task(checked: try container.decode(Bool.self, forKey: .checked))
        case "link":
            payload = .link(dest: try container.decode(FixtureRange.self, forKey: .dest))
        case "image":
            payload = .image(
                src: try container.decode(FixtureRange.self, forKey: .src),
                wiki: try container.decode(Bool.self, forKey: .wiki)
            )
        case "fence":
            payload = .fence(info: try container.decode(FixtureRange.self, forKey: .info))
        case "wikilink":
            payload = .wikilink(target: try container.decode(FixtureRange.self, forKey: .target))
        case "heading":
            payload = .heading(
                level: try container.decode(UInt8.self, forKey: .level),
                text: try container.decode(FixtureRange.self, forKey: .text)
            )
        default:
            // A kind the mirror can't model is drift between the Rust core and
            // this suite — exactly what the fixture gate exists to catch, so it
            // fails loudly instead of being skipped.
            throw DecodingError.dataCorruptedError(
                forKey: .kind, in: container, debugDescription: "unknown element kind \(kind)"
            )
        }
    }
}

struct FixtureSegment: Decodable {
    let range: FixtureRange
    let kinds: [String]
    let concealed: Bool
}

struct FixtureRange: Decodable {
    let start: UInt32
    let end: UInt32
    let utf16Start: UInt32
    let utf16End: UInt32
}

/// Fixture kind-name strings -> `KamiKindSet`. Test-only:
/// production code never needs to parse kind names from strings.
private let kindByName: [String: KamiKindSet] = [
    "body": .body,
    "heading1": .heading1, "heading2": .heading2, "heading3": .heading3,
    "heading4": .heading4, "heading5": .heading5, "heading6": .heading6,
    "strong": .strong, "emphasis": .emphasis, "strikethrough": .strikethrough,
    "code_span": .codeSpan, "code_block": .codeBlock, "fence_info": .fenceInfo,
    "blockquote": .blockquote, "list_bullet": .listBullet, "list_ordinal": .listOrdinal,
    "task_marker": .taskMarker, "link": .link, "image": .image, "table": .table,
    "thematic_break": .thematicBreak, "marker": .marker, "html_raw": .htmlRaw
]

extension KamiKindSet {
    init(fixtureNames names: [String]) {
        self = names.reduce(into: []) { result, name in
            if let kind = kindByName[name] { result.insert(kind) }
        }
    }
}
