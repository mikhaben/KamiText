//! Typing behaviors: newline continuation and task toggling.
//! Plans are suggestions — the adapter applies them via `apply_edit`.

use crate::document::Document;
use crate::parse::TaskBox;
use crate::types::{ByteRange, EditPlan};

/// Lexical shape of the caret line's prefix.
struct LinePrefix {
    /// End of the whole prefix (after the marker/task-box and its one
    /// following space), i.e. where item content starts.
    content_start: u32,
    /// Copied verbatim into the continuation: indent + quote markers.
    quotes_end: u32,
    /// The list marker, if any.
    marker: Option<Marker>,
    has_quotes: bool,
}

enum Marker {
    Bullet(u8),
    Ordinal(u64, u8),
    /// Bullet char of a task item; continuation inserts `<bullet> [ ] `.
    Task(u8),
}

fn scan_prefix(text: &str, line: ByteRange, task_lists: bool) -> LinePrefix {
    let bytes = text.as_bytes();
    let mut p = line.start;

    // Leading indent + any number of `> ` quote prefixes (each: up to 3
    // spaces, '>', optional space).
    let mut quotes_end = line.start;
    let mut has_quotes = false;
    loop {
        let mut q = p;
        let mut spaces = 0;
        while q < line.end && bytes[q as usize] == b' ' && spaces < 3 {
            q += 1;
            spaces += 1;
        }
        if q < line.end && bytes[q as usize] == b'>' {
            q += 1;
            if q < line.end && bytes[q as usize] == b' ' {
                q += 1;
            }
            p = q;
            quotes_end = q;
            has_quotes = true;
        } else {
            break;
        }
    }

    // List-item indent after the quotes.
    let mut m = p;
    while m < line.end && bytes[m as usize] == b' ' {
        m += 1;
    }

    let mut marker = None;
    let mut content_start = quotes_end.max(line.start);
    if m < line.end && matches!(bytes[m as usize], b'-' | b'*' | b'+') {
        let bullet = bytes[m as usize];
        let after = m + 1;
        if after == line.end || bytes[after as usize] == b' ' {
            let mut c = after;
            if c < line.end && bytes[c as usize] == b' ' {
                c += 1;
            }
            // Task box directly after the bullet: `[ ]` / `[x]` + space/EOL.
            let is_task = task_lists
                && c + 3 <= line.end
                && bytes[c as usize] == b'['
                && matches!(bytes[c as usize + 1], b' ' | b'x' | b'X')
                && bytes[c as usize + 2] == b']'
                && (c + 3 == line.end || bytes[c as usize + 3] == b' ');
            if is_task {
                let mut cs = c + 3;
                if cs < line.end && bytes[cs as usize] == b' ' {
                    cs += 1;
                }
                marker = Some(Marker::Task(bullet));
                content_start = cs;
            } else {
                marker = Some(Marker::Bullet(bullet));
                content_start = c;
            }
        }
    } else {
        let mut d = m;
        while d < line.end && bytes[d as usize].is_ascii_digit() && d - m < 9 {
            d += 1;
        }
        if d > m && d < line.end && matches!(bytes[d as usize], b'.' | b')') {
            let after = d + 1;
            if after == line.end || bytes[after as usize] == b' ' {
                let mut c = after;
                if c < line.end && bytes[c as usize] == b' ' {
                    c += 1;
                }
                let value: u64 = text[m as usize..d as usize].parse().unwrap_or(0);
                marker = Some(Marker::Ordinal(value, bytes[d as usize]));
                content_start = c;
            }
        }
    }

    // The indent between quotes and the marker is replicated verbatim, so the
    // continuation prefix copies text[line.start..marker_start] and appends a
    // freshly built marker. Record where that verbatim part ends.
    let verbatim_end = if marker.is_some() { m } else { quotes_end };
    LinePrefix {
        content_start: content_start.max(verbatim_end),
        quotes_end: verbatim_end,
        marker,
        has_quotes,
    }
}

pub fn newline_plan(
    doc: &Document,
    verbatim_blocks: &[ByteRange],
    task_lists: bool,
    at: u32,
) -> Option<EditPlan> {
    let text = doc.text();
    let line_idx = doc.line_of(at);
    let line = doc.line_content_range(line_idx);

    // No lexical continuation inside code/HTML blocks.
    if verbatim_blocks
        .iter()
        .any(|b| !b.is_empty() && b.start <= line.start && line.start < b.end)
    {
        return None;
    }

    let prefix = scan_prefix(text, line, task_lists);
    if prefix.marker.is_none() && !prefix.has_quotes {
        return None;
    }

    let rest = &text[prefix.content_start as usize..line.end as usize];
    let empty = rest.bytes().all(|b| b == b' ' || b == b'\t');

    if empty {
        // Exit-on-empty: remove the empty marker line's prefix.
        return Some(EditPlan {
            edits: vec![(ByteRange::new(line.start, line.end), String::new())],
            caret: line.start,
        });
    }

    // Continue: newline + verbatim (indent+quotes+item indent) + fresh marker.
    let mut inserted = String::with_capacity(
        1 + (prefix.quotes_end - line.start) as usize + 8,
    );
    inserted.push('\n');
    inserted.push_str(&text[line.start as usize..prefix.quotes_end as usize]);
    match prefix.marker {
        Some(Marker::Bullet(b)) => {
            inserted.push(b as char);
            inserted.push(' ');
        }
        Some(Marker::Task(b)) => {
            inserted.push(b as char);
            inserted.push_str(" [ ] ");
        }
        Some(Marker::Ordinal(n, punct)) => {
            // Next number, renumber-free: following lines are left alone.
            inserted.push_str(&(n + 1).to_string());
            inserted.push(punct as char);
            inserted.push(' ');
        }
        None => {}
    }
    let caret = at + inserted.len() as u32;
    Some(EditPlan {
        edits: vec![(ByteRange::new(at, at), inserted)],
        caret,
    })
}

pub fn toggle_task_plan(task_boxes: &[TaskBox], doc_len: u32, at: u32) -> Option<EditPlan> {
    // Innermost task item containing the caret (nested tasks: smallest range
    // wins). Half-open per the §4.2 boundary rule — an offset at the boundary
    // of two adjacent items belongs to the one it starts — except at the very
    // end of the document, so a caret after a final, newline-less item still
    // addresses it.
    let hit = task_boxes
        .iter()
        .filter(|t| {
            t.item.start <= at && (at < t.item.end || (at == t.item.end && at == doc_len))
        })
        .min_by_key(|t| t.item.len())?;
    let replacement = if hit.checked { "[ ]" } else { "[x]" };
    Some(EditPlan {
        edits: vec![(hit.boxx, replacement.to_string())],
        caret: at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse, ParseOut};
    use crate::types::Extensions;

    fn boxes(text: &str) -> Vec<TaskBox> {
        let mut po = ParseOut::default();
        parse(text, Extensions::all(), &mut po);
        po.task_boxes
    }

    #[test]
    fn toggle_boundary_belongs_to_the_item_it_starts() {
        let text = "- [ ] a\n- [ ] b\n";
        let tb = boxes(text);
        // Byte 8 starts item 1's line; it must toggle item 1's box, not item 0's.
        let plan = toggle_task_plan(&tb, text.len() as u32, 8).unwrap();
        assert_eq!(plan.edits[0].0.start, 10);
        // End of document still addresses the final item.
        let plan = toggle_task_plan(&tb, text.len() as u32, text.len() as u32).unwrap();
        assert_eq!(plan.edits[0].0.start, 10);
    }

    #[test]
    fn crlf_exit_on_empty() {
        let doc = crate::document::Document::new("- one\r\n- \r\n");
        // Caret on the empty item (line 1 content is "- "): exit, not continue.
        let plan = newline_plan(&doc, &[], true, 9).unwrap();
        assert_eq!(plan.edits[0].1, "");
        assert_eq!(plan.edits[0].0, ByteRange::new(7, 9));
    }
}
