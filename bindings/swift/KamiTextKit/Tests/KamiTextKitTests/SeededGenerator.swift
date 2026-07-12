import Foundation

/// Deterministic splitmix64 RNG, implemented standalone — no shared
/// dependency. Same seed always produces the same op
/// sequence, so a fuzz divergence is reproducible from (seed, iteration
/// index) alone.
struct SeededGenerator {
    private var state: UInt64

    init(seed: UInt64) {
        state = seed
    }

    mutating func nextUInt64() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    /// Uniform in `0..<bound`; `bound <= 0` always returns 0.
    mutating func nextInt(upperBound bound: Int) -> Int {
        guard bound > 0 else { return 0 }
        return Int(nextUInt64() % UInt64(bound))
    }

    mutating func nextDouble() -> Double {
        Double(nextUInt64() >> 11) * (1.0 / 9_007_199_254_740_992.0) // 2^53
    }

    mutating func chance(_ probability: Double) -> Bool {
        nextDouble() < probability
    }

    mutating func choice<T>(_ items: [T]) -> T {
        items[nextInt(upperBound: items.count)]
    }
}

/// Markdown-shaped fragment pool (M4 plan item 2): newlines, emphasis/
/// strong/code-span/strikethrough delimiters, headings, task markers,
/// links, a fence, an astral emoji and a mid-word astral case, and CJK —
/// the same byte-vs-UTF-16 and marker-scope surfaces the `element-reveal-*`
/// fixtures pin.
private let fuzzFragments: [String] = [
    "\n", "\n\n", "**bold**", "*italic*", "`code`", "~~strike~~",
    "# Heading\n", "## Sub\n", "- [ ] task\n", "- [x] done\n",
    "[t](u)", "[link](https://example.com)", "> quote\n",
    "```\ncode block\n```\n", "🎉", "sun🌞shine", "日本語", "見出し",
    "plain text ", "- bullet\n"
]

/// A random markdown-shaped insertion fragment (M4 plan item 2).
func randomFragment(_ rng: inout SeededGenerator) -> String {
    rng.choice(fuzzFragments)
}

/// Deterministic synthetic document of roughly `approximateBytes` UTF-8
/// bytes (M4 plan item 2), built from the same fragment pool the edit ops
/// draw from so the corpus and the mutations exercise consistent structure.
func makeLargeMarkdown(approximateBytes: Int, seed: UInt64) -> String {
    var rng = SeededGenerator(seed: seed)
    var result = ""
    while result.utf8.count < approximateBytes {
        result += randomFragment(&rng)
    }
    return result
}
