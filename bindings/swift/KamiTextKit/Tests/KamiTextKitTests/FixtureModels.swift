@testable import KamiTextKit

/// Decodable mirror of the `fixtures/*.json` schema. Only the
/// fields Gate 2 needs (segments-only conformance) are modeled; unused JSON
/// keys (`schema`, `expect.patches`, `expect.elements`) are simply ignored by
/// `JSONDecoder`.
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
    let text: String
    let lenBytes: UInt32
    let lenUtf16: UInt32
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
