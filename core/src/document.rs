//! Text buffer, edit application and physical-line index.
//!
//! A "physical line" is the text between `\n` boundaries; a line's range
//! includes its trailing `\n`. A document ending in `\n` has a
//! trailing empty line.

use crate::offsets::Utf16Index;
use crate::types::{ByteRange, KamiError};

pub struct Document {
    text: String,
    /// Byte offset of the start of every line: `[0, pos-after-each-\n...]`.
    line_starts: Vec<u32>,
    utf16: Utf16Index,
}

impl Document {
    pub fn new(text: &str) -> Self {
        let mut doc = Self {
            text: text.to_owned(),
            line_starts: Vec::new(),
            utf16: Utf16Index::new(text),
        };
        doc.rebuild_lines();
        doc
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len_bytes(&self) -> u32 {
        self.text.len() as u32
    }

    pub fn len_utf16(&self) -> u32 {
        self.utf16.len_utf16()
    }

    /// Strict validation for mutating calls: in bounds, ordered,
    /// scalar-aligned. Never repairs.
    pub fn validate_range(&self, range: ByteRange) -> Result<(), KamiError> {
        let len = self.text.len() as u32;
        if range.start > range.end || range.end > len {
            return Err(KamiError::InvalidRange);
        }
        if !self.text.is_char_boundary(range.start as usize)
            || !self.text.is_char_boundary(range.end as usize)
        {
            return Err(KamiError::InvalidRange);
        }
        Ok(())
    }

    pub fn validate_offset(&self, offset: u32) -> Result<(), KamiError> {
        self.validate_range(ByteRange::new(offset, offset))
    }

    /// Applies a validated edit. Caller must have run [`Self::validate_range`].
    /// Both the UTF-16 index and the line index are incremental: the UTF-16
    /// splice patches only the chunks touching the edit, and the line splice
    /// (below) rewrites only the affected `line_starts` span, shifting the
    /// tail by an O(lines after the edit) `u32` add per entry rather than
    /// rescanning the document.
    pub fn apply(&mut self, range: ByteRange, replacement: &str) {
        self.utf16
            .apply_edit(&self.text, range.start, range.end, replacement);
        self.text
            .replace_range(range.start as usize..range.end as usize, replacement);
        self.splice_lines(range.start, range.end, replacement.len() as u32);
    }

    fn rebuild_lines(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (i, b) in self.text.bytes().enumerate() {
            if b == b'\n' {
                self.line_starts.push(i as u32 + 1);
            }
        }
    }

    /// Splices the line index for `text[start..end] -> replacement` of length
    /// `rep_len`. Called AFTER the text splice (`self.text` is post-edit).
    /// Entries ≤ start are untouched (a line starting exactly at `start` still
    /// starts there — the byte before it is still `\n` or BOF); entries in
    /// (start, end] belonged to removed `\n`s; entries > end shift by the length
    /// delta; each `\n` in the replacement contributes `start + i + 1`.
    fn splice_lines(&mut self, start: u32, end: u32, rep_len: u32) {
        let delta = rep_len as i64 - i64::from(end - start);
        let lo = self.line_starts.partition_point(|&e| e <= start);
        let hi = self.line_starts.partition_point(|&e| e <= end);
        let inserted: Vec<u32> = self.text[start as usize..(start + rep_len) as usize]
            .bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .map(|(i, _)| start + i as u32 + 1)
            .collect();
        let tail = lo + inserted.len();
        self.line_starts.splice(lo..hi, inserted);
        for e in &mut self.line_starts[tail..] {
            *e = (i64::from(*e) + delta) as u32;
        }
    }

    /// Index of the line containing `offset`. An offset at a line boundary
    /// belongs to the line it starts; `offset == len` addresses the last line.
    pub fn line_of(&self, offset: u32) -> usize {
        self.line_starts.partition_point(|&ls| ls <= offset) - 1
    }

    /// Byte range of line `idx`, including its trailing `\n` if present.
    pub fn line_range(&self, idx: usize) -> ByteRange {
        let start = self.line_starts[idx];
        let end = self
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.text.len() as u32);
        ByteRange::new(start, end)
    }

    /// Content range of line `idx`, excluding its trailing `\n` (and the `\r`
    /// of a CRLF ending — pulldown treats `\r\n` as one line ending, so
    /// lexical scanners must not see the `\r` as content).
    pub fn line_content_range(&self, idx: usize) -> ByteRange {
        let r = self.line_range(idx);
        let bytes = self.text.as_bytes();
        let mut end = r.end;
        if end > r.start && bytes[end as usize - 1] == b'\n' {
            end -= 1;
            if end > r.start && bytes[end as usize - 1] == b'\r' {
                end -= 1;
            }
        }
        ByteRange::new(r.start, end)
    }

    #[cfg(test)]
    pub fn line_starts(&self) -> &[u32] {
        &self.line_starts
    }

    pub fn byte_to_utf16(&self, offset: u32) -> u32 {
        self.utf16.byte_to_utf16(&self.text, offset)
    }

    pub fn utf16_to_byte(&self, offset: u32) -> u32 {
        self.utf16.utf16_to_byte(&self.text, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_basics() {
        let d = Document::new("ab\ncd\n");
        assert_eq!(d.line_starts(), &[0, 3, 6]);
        assert_eq!(d.line_of(0), 0);
        assert_eq!(d.line_of(2), 0);
        assert_eq!(d.line_of(3), 1);
        assert_eq!(d.line_of(6), 2); // trailing empty line
        assert_eq!(d.line_range(0), ByteRange::new(0, 3));
        assert_eq!(d.line_content_range(0), ByteRange::new(0, 2));
        assert_eq!(d.line_range(2), ByteRange::new(6, 6));
    }

    #[test]
    fn line_index_no_trailing_newline() {
        let d = Document::new("ab\ncd");
        assert_eq!(d.line_starts(), &[0, 3]);
        assert_eq!(d.line_of(5), 1);
        assert_eq!(d.line_range(1), ByteRange::new(3, 5));
        assert_eq!(d.line_content_range(1), ByteRange::new(3, 5));
    }

    #[test]
    fn empty_document() {
        let d = Document::new("");
        assert_eq!(d.line_starts(), &[0]);
        assert_eq!(d.line_of(0), 0);
        assert_eq!(d.line_range(0), ByteRange::new(0, 0));
    }

    #[test]
    fn validate_rejects_scalar_split() {
        let d = Document::new("a😀b");
        assert!(d.validate_range(ByteRange::new(1, 5)).is_ok());
        assert_eq!(
            d.validate_range(ByteRange::new(2, 5)),
            Err(KamiError::InvalidRange)
        );
        assert_eq!(
            d.validate_range(ByteRange::new(0, 7)),
            Err(KamiError::InvalidRange)
        );
        assert_eq!(
            d.validate_range(ByteRange::new(3, 1)),
            Err(KamiError::InvalidRange)
        );
    }

    #[test]
    fn apply_edit_updates_lines() {
        let mut d = Document::new("ab\ncd");
        d.apply(ByteRange::new(2, 2), "\nX");
        assert_eq!(d.text(), "ab\nX\ncd");
        assert_eq!(d.line_starts(), &[0, 3, 5]);
    }

    #[test]
    fn splice_spans_newline() {
        let mut d = Document::new("ab\ncd\nef");
        assert_eq!(d.line_starts(), &[0, 3, 6]);
        d.apply(ByteRange::new(1, 4), "X");
        assert_eq!(d.text(), "aXd\nef");
        assert_eq!(d.line_starts(), &[0, 4]);
    }

    #[test]
    fn splice_inserts_three_newlines() {
        let mut d = Document::new("abcd");
        d.apply(ByteRange::new(2, 2), "\n\n\n");
        assert_eq!(d.text(), "ab\n\n\ncd");
        assert_eq!(d.line_starts(), &[0, 3, 4, 5]);
    }

    #[test]
    fn splice_deletes_all_newlines() {
        let mut d = Document::new("a\nb\nc\n");
        assert_eq!(d.line_starts(), &[0, 2, 4, 6]);
        d.apply(ByteRange::new(0, 6), "abc");
        assert_eq!(d.text(), "abc");
        assert_eq!(d.line_starts(), &[0]);
    }

    #[test]
    fn splice_at_offset_zero() {
        let mut d = Document::new("abc\ndef");
        d.apply(ByteRange::new(0, 0), "X\n");
        assert_eq!(d.text(), "X\nabc\ndef");
        assert_eq!(d.line_starts(), &[0, 2, 6]);
    }

    #[test]
    fn splice_at_eof() {
        let mut d = Document::new("abc\ndef");
        d.apply(ByteRange::new(7, 7), "\nX");
        assert_eq!(d.text(), "abc\ndef\nX");
        assert_eq!(d.line_starts(), &[0, 4, 8]);
    }

    #[test]
    fn splice_pure_insert_at_line_start_boundary_does_not_shift_entry() {
        let mut d = Document::new("ab\ncd");
        assert_eq!(d.line_starts(), &[0, 3]);
        d.apply(ByteRange::new(3, 3), "XY");
        assert_eq!(d.text(), "ab\nXYcd");
        // The entry at `start` (3) must not shift: the line still starts at
        // byte 3 (now "X"), unaffected by content inserted from that point.
        assert_eq!(d.line_starts(), &[0, 3]);
    }

    #[test]
    fn splice_crlf_content() {
        let mut d = Document::new("ab\r\ncd\r\n");
        assert_eq!(d.line_starts(), &[0, 4, 8]);
        d.apply(ByteRange::new(4, 4), "X\r\nY");
        assert_eq!(d.text(), "ab\r\nX\r\nYcd\r\n");
        assert_eq!(d.line_starts(), &[0, 4, 7, 12]);
    }

    #[test]
    fn splice_empty_replacement_removes_newline() {
        let mut d = Document::new("ab\ncd\nef");
        assert_eq!(d.line_starts(), &[0, 3, 6]);
        d.apply(ByteRange::new(2, 3), "");
        assert_eq!(d.text(), "abcd\nef");
        assert_eq!(d.line_starts(), &[0, 5]);
    }

    #[test]
    fn splice_whole_document_replacement_entry_zero_survives() {
        let mut d = Document::new("ab\ncd");
        d.apply(ByteRange::new(0, 5), "xy\nz\nw");
        assert_eq!(d.text(), "xy\nz\nw");
        assert_eq!(d.line_starts(), &[0, 3, 5]);
    }

    use proptest::prelude::*;

    /// Replacement atoms exercising `\n`, `\r\n`, empty lines, emoji and
    /// multi-byte scripts.
    const SPLICE_ATOMS: &[&str] = &[
        "", "a", "\n", "\n\n", "\r\n", "ab\r\ncd\r\n", "😀", "😀\n", "日本語\n",
        "\u{200D}", "x\ny\nz\n", "\r", " ", "line\n",
    ];

    #[derive(Debug, Clone, Copy)]
    enum SpliceOp {
        Insert(usize, usize),
        Delete(usize, usize),
        Replace(usize, usize, usize),
    }

    fn splice_op_strategy() -> impl Strategy<Value = SpliceOp> {
        prop_oneof![
            (any::<usize>(), 0..SPLICE_ATOMS.len()).prop_map(|(p, a)| SpliceOp::Insert(p, a)),
            (any::<usize>(), 0..20usize).prop_map(|(p, l)| SpliceOp::Delete(p, l)),
            (any::<usize>(), 0..20usize, 0..SPLICE_ATOMS.len())
                .prop_map(|(p, l, a)| SpliceOp::Replace(p, l, a)),
        ]
    }

    /// Clamps an arbitrary position to a scalar boundary at or below it.
    fn align(text: &str, pos: usize) -> u32 {
        let mut p = pos.min(text.len());
        while !text.is_char_boundary(p) {
            p -= 1;
        }
        p as u32
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 512,
            ..ProptestConfig::default()
        })]

        /// After any edit sequence, the spliced line index must equal a
        /// from-scratch recompute (`Document` is not `Clone`, so a fresh
        /// construction over the current text is the oracle).
        #[test]
        fn splice_matches_fresh_rebuild(
            seed in prop::sample::select(vec![
                "",
                "a\nb\nc\n",
                "line one\r\nline two\r\n\r\nline four",
                "日本語\n\n😀🏳️‍🌈\n",
                "no newlines at all",
                "\n\n\n",
            ]),
            ops in prop::collection::vec(splice_op_strategy(), 1..30),
        ) {
            let mut d = Document::new(seed);
            for op in ops {
                let len = d.text().len();
                let (start, end, replacement) = match op {
                    SpliceOp::Insert(p, a) => {
                        let at = align(d.text(), p % (len + 1));
                        (at, at, SPLICE_ATOMS[a])
                    }
                    SpliceOp::Delete(p, l) => {
                        let s = align(d.text(), p % (len + 1));
                        let e = align(d.text(), (s as usize + l).min(len));
                        (s.min(e), s.max(e), "")
                    }
                    SpliceOp::Replace(p, l, a) => {
                        let s = align(d.text(), p % (len + 1));
                        let e = align(d.text(), (s as usize + l).min(len));
                        (s.min(e), s.max(e), SPLICE_ATOMS[a])
                    }
                };
                d.apply(ByteRange::new(start, end), replacement);
                let fresh = Document::new(d.text());
                prop_assert_eq!(d.line_starts(), fresh.line_starts());
            }
        }
    }
}
