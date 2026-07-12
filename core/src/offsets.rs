//! Checkpointed byte ↔ UTF-16 index.
//!
//! The document is divided into chunks of ~4 KB, each ending on a scalar
//! boundary. Cumulative (byte, utf16) counts are kept at chunk boundaries and
//! patched incrementally on edit: only the chunks touching the edit are
//! recounted. Any conversion is a binary search over checkpoints plus one
//! bounded chunk scan.

/// Target chunk size in bytes. Chunks may run slightly over to end on a
/// scalar boundary.
const CHUNK: usize = 4096;

pub struct Utf16Index {
    /// Per-chunk sizes: (byte length, utf16 length). Concatenated chunks cover
    /// the whole document; empty document has zero chunks.
    chunks: Vec<(u32, u32)>,
    /// Cumulative sums, one entry per chunk boundary: `cum[0] == (0, 0)`,
    /// `cum[chunks.len()] == (len_bytes, len_utf16)`.
    cum: Vec<(u32, u32)>,
}

fn utf16_len(s: &str) -> u32 {
    if s.is_ascii() {
        s.len() as u32
    } else {
        s.chars().map(|c| c.len_utf16() as u32).sum()
    }
}

/// Splits `s` into chunk sizes of ~CHUNK bytes, scalar-aligned.
fn chunk_sizes(s: &str, out: &mut Vec<(u32, u32)>) {
    let mut pos = 0usize;
    while pos < s.len() {
        let mut end = (pos + CHUNK).min(s.len());
        while !s.is_char_boundary(end) {
            end += 1;
        }
        out.push(((end - pos) as u32, utf16_len(&s[pos..end])));
        pos = end;
    }
}

impl Utf16Index {
    pub fn new(text: &str) -> Self {
        let mut idx = Self {
            chunks: Vec::new(),
            cum: Vec::new(),
        };
        chunk_sizes(text, &mut idx.chunks);
        idx.rebuild_cum();
        idx
    }

    fn rebuild_cum(&mut self) {
        self.cum.clear();
        self.cum.reserve(self.chunks.len() + 1);
        let (mut b, mut u) = (0u32, 0u32);
        self.cum.push((0, 0));
        for &(cb, cu) in &self.chunks {
            b += cb;
            u += cu;
            self.cum.push((b, u));
        }
    }

    pub fn len_utf16(&self) -> u32 {
        self.cum.last().map(|&(_, u)| u).unwrap_or(0)
    }

    /// Incrementally patches the index for `old_text[start..end] -> replacement`.
    /// Called BEFORE the text splice (`old_text` is the pre-edit text).
    pub fn apply_edit(&mut self, old_text: &str, start: u32, end: u32, replacement: &str) {
        let old_len = old_text.len();
        let delta = replacement.len() as i64 - (end - start) as i64;
        let new_len = (old_len as i64 + delta) as usize;
        if self.chunks.is_empty() || new_len <= 2 * CHUNK {
            // Small doc or previously empty: full rebuild is a bounded scan.
            let mut new_text = String::with_capacity(new_len);
            new_text.push_str(&old_text[..start as usize]);
            new_text.push_str(replacement);
            new_text.push_str(&old_text[end as usize..]);
            self.chunks.clear();
            chunk_sizes(&new_text, &mut self.chunks);
            self.rebuild_cum();
            return;
        }

        // First chunk whose byte span contains `start` (a boundary offset
        // belongs to the chunk starting there, so inserts at a boundary land
        // in the following chunk; `start == len` lands in the last chunk).
        let a = self
            .cum
            .partition_point(|&(b, _)| b <= start)
            .saturating_sub(1)
            .min(self.chunks.len() - 1);
        // Last chunk touching the edit: the one containing `end - 1`
        // (or `a` itself for a pure insert).
        let b = if end > start {
            self.cum
                .partition_point(|&(cb, _)| cb < end)
                .saturating_sub(1)
                .min(self.chunks.len() - 1)
        } else {
            a
        };

        let region_start = self.cum[a].0 as usize;
        let region_old_end = self.cum[b + 1].0 as usize;
        // Recount the affected region against the post-edit bytes.
        let mut region = String::with_capacity(
            (region_old_end as i64 - region_start as i64 + delta) as usize,
        );
        region.push_str(&old_text[region_start..start as usize]);
        region.push_str(replacement);
        region.push_str(&old_text[end as usize..region_old_end]);

        let mut replacement_chunks = Vec::new();
        chunk_sizes(&region, &mut replacement_chunks);
        self.chunks.splice(a..=b, replacement_chunks);
        self.rebuild_cum();
    }

    /// Byte offset → UTF-16 offset. A mid-scalar byte offset floors to the
    /// scalar's start.
    pub fn byte_to_utf16(&self, text: &str, offset: u32) -> u32 {
        let len = text.len() as u32;
        if offset >= len {
            return self.len_utf16();
        }
        let i = self
            .cum
            .partition_point(|&(b, _)| b <= offset)
            .saturating_sub(1);
        let (chunk_byte, chunk_u16) = self.cum[i];
        let mut pos = chunk_byte;
        let mut u16 = chunk_u16;
        for ch in text[chunk_byte as usize..].chars() {
            let w = ch.len_utf8() as u32;
            if pos + w > offset {
                break;
            }
            pos += w;
            u16 += ch.len_utf16() as u32;
        }
        u16
    }

    /// UTF-16 offset → byte offset, rounding down to a scalar start.
    pub fn utf16_to_byte(&self, text: &str, offset: u32) -> u32 {
        if offset >= self.len_utf16() {
            return text.len() as u32;
        }
        let i = self
            .cum
            .partition_point(|&(_, u)| u <= offset)
            .saturating_sub(1);
        let (chunk_byte, chunk_u16) = self.cum[i];
        let mut pos = chunk_byte;
        let mut u16 = chunk_u16;
        for ch in text[chunk_byte as usize..].chars() {
            let wu = ch.len_utf16() as u32;
            if u16 + wu > offset {
                break;
            }
            pos += ch.len_utf8() as u32;
            u16 += wu;
        }
        pos
    }

    #[cfg(test)]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_b2u(text: &str, offset: u32) -> u32 {
        let mut pos = 0u32;
        let mut u16 = 0u32;
        for ch in text.chars() {
            let w = ch.len_utf8() as u32;
            if pos + w > offset {
                break;
            }
            pos += w;
            u16 += ch.len_utf16() as u32;
        }
        u16
    }

    fn naive_u2b(text: &str, offset: u32) -> u32 {
        let mut pos = 0u32;
        let mut u16 = 0u32;
        for ch in text.chars() {
            let wu = ch.len_utf16() as u32;
            if u16 + wu > offset {
                break;
            }
            pos += ch.len_utf8() as u32;
            u16 += wu;
        }
        pos
    }

    fn assert_matches_naive(text: &str, idx: &Utf16Index) {
        for off in 0..=text.len() as u32 + 2 {
            assert_eq!(idx.byte_to_utf16(text, off), naive_b2u(text, off), "b2u {off} in {text:?}");
        }
        let total = naive_b2u(text, text.len() as u32);
        assert_eq!(idx.len_utf16(), total);
        for off in 0..=total + 2 {
            let expect = if off >= total {
                text.len() as u32
            } else {
                naive_u2b(text, off)
            };
            assert_eq!(idx.utf16_to_byte(text, off), expect, "u2b {off} in {text:?}");
        }
    }

    #[test]
    fn conversions_mixed_scalars() {
        // ASCII, emoji (astral: 4 bytes / 2 units), CJK (3 bytes / 1 unit), ZWJ family.
        let zwj_family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}x"; // 👨‍👩‍👧x
        for text in ["", "abc", "a😀b", "日本語", zwj_family, "a\u{200D}b", "😀😀😀"] {
            let idx = Utf16Index::new(text);
            assert_matches_naive(text, &idx);
        }
    }

    #[test]
    fn incremental_edit_matches_rebuild() {
        // Large enough to hit the multi-chunk path.
        let seed = "abc😀日本語 def\n";
        let mut text: String = seed.repeat(600); // ~10 KB
        let mut idx = Utf16Index::new(&text);
        assert!(idx.chunk_count() >= 2);

        let edits: &[(u32, u32, &str)] = &[
            (0, 0, "😀"),
            (5000, 5010, ""),
            (200, 200, &"x".repeat(5000)),
            (1, 9000, "日"),
        ];
        for &(mut s, mut e, repl) in edits {
            while !text.is_char_boundary(s as usize) {
                s -= 1;
            }
            while !text.is_char_boundary(e as usize) {
                e -= 1;
            }
            let (s, e) = (s.min(e), s.max(e));
            idx.apply_edit(&text, s, e, repl);
            text.replace_range(s as usize..e as usize, repl);
            let fresh = Utf16Index::new(&text);
            assert_eq!(idx.len_utf16(), fresh.len_utf16());
            // Spot-check a spread of offsets rather than every byte (10 KB doc).
            for off in (0..=text.len() as u32).step_by(37) {
                assert_eq!(
                    idx.byte_to_utf16(&text, off),
                    fresh.byte_to_utf16(&text, off)
                );
            }
        }
    }

    #[test]
    fn edit_to_empty() {
        let mut text = String::from("hello");
        let mut idx = Utf16Index::new(&text);
        idx.apply_edit(&text, 0, 5, "");
        text.clear();
        assert_eq!(idx.len_utf16(), 0);
        assert_eq!(idx.byte_to_utf16(&text, 0), 0);
        assert_eq!(idx.utf16_to_byte(&text, 0), 0);
    }
}
