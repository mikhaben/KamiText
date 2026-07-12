# Emoji & ZWJ stress document

Synthetic-structured stress corpus: multi-scalar graphemes placed directly against markdown delimiters, where UTF-8/UTF-16 width math earns its keep. Every section pairs a grapheme class with marker adjacency.

## 1. ZWJ family sequences

The classic four-person family 👨‍👩‍👧‍👦 sits mid-sentence, **bold 👨‍👩‍👧‍👦 inside strong**, *emphasis 👩‍👩‍👦 tail*, and one at the very end of a line 👨‍👨‍👧‍👧
A couple with heart: 👩‍❤️‍👨 and the kiss variant 👩‍❤️‍💋‍👨 — both cross ZWJ + VS16.

- list item starting with 👨‍👩‍👧‍👦 family
  - nested: **👩‍🚀 astronaut** profession sequence
    - deeper: `code span with 👨‍🔬 inside`
      - fourth level: *emphasis ending in ZWJ sequence 🧑‍🤝‍🧑*

## 2. Skin tones & modifiers

Waving hands across all tones: 👋 👋🏻 👋🏼 👋🏽 👋🏾 👋🏿 — and **bold 🤝🏽 handshake**.
Profession + tone + ZWJ: 👩🏽‍🚀 👨🏿‍⚕️ 🧑🏻‍💻 — the last one hugs a closing delimiter: *🧑🏻‍💻*

## 3. Flags & keycaps

Regional-indicator pairs: 🇺🇦 🇵🇱 🇯🇵 🇸🇦 — tag-sequence flag 🏴󠁧󠁢󠁳󠁣󠁴󠁿 and rainbow 🏳️‍🌈 plus pirate 🏴‍☠️.
Keycaps: 1️⃣ 2️⃣ #️⃣ *️⃣ — an asterisk keycap beside a closed emphasis span: *before* *️⃣ after.
(A keycap whose base `*` doubles as the closing emphasis delimiter is deliberately absent: AppKit font-fixing cannot split a composed sequence, so concealing that delimiter byte diverges — PLATFORM_BUGS.md, composed-sequence concealment.)

> Blockquote with 🏳️‍⚧️ flag and `inline 🇪🇺 code`
> - [ ] task with family 👨‍👩‍👧 unchecked
> - [x] task with astronaut 👩🏼‍🚀 checked

## 4. Combining marks & variation selectors

Emoji-presentation selectors: ☺️ and ✔️ mid-sentence. Digits with enclosing keycap under strikethrough: ~~3️⃣ struck~~.
(Text-presentation forms — U+263A/U+2714 + VS15 — are deliberately absent: AppKit substitutes a history-dependent concrete font for glyphs the system meta-font lacks, so their attribute reads are unstable across storages; PLATFORM_BUGS.md #4.)
Devanagari cluster क्षि and Thai cluster เกี่ยว beside emphasis: *क्षि* and **เกี่ยว**.

## 5. Marker-adjacent edge lattice

**👨‍👩‍👧‍👦**bold-hug, *👋🏽*emphasis-hug, `👩‍🚀`code-hug, [🏳️‍🌈 link](https://example.com/🇺🇳), ![👨‍🔬 alt](https://example.com/e.png)

| grapheme | class | scalars |
|---|---|---|
| 👨‍👩‍👧‍👦 | ZWJ ×3 | 7 |
| 👋🏿 | tone | 2 |
| 🏴󠁧󠁢󠁳󠁣󠁴󠁿 | tag seq | 7 |
| 1️⃣ | keycap | 3 |
