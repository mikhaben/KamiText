//! Conformance fixture exporter.
//!
//! `cargo run --bin export-fixtures` regenerates `fixtures/*.json` at the
//! repo root. Fixtures are generated from the engine itself and committed;
//! adapters replay them through the FFI and must reproduce the outputs.
//!
//! JSON is hand-serialized: the schema is small, output must be byte-stable
//! across runs, and the core crate stays dependency-light.

use kamitext::{
    ByteRange, ElementKind, Engine, EngineOptions, Extensions, Kind, Patch, RevealMode,
};
use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Clone, Copy)]
enum Op {
    Edit(u32, u32, &'static str),
    Selection(u32, u32),
}

struct Fixture {
    name: &'static str,
    options: EngineOptions,
    text: &'static str,
    ops: Vec<Op>,
}

fn default_opts() -> EngineOptions {
    EngineOptions::default()
}

fn element_opts() -> EngineOptions {
    EngineOptions {
        reveal: RevealMode::Element,
        ..Default::default()
    }
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "composition-heading-strong",
            options: default_opts(),
            text: "# **word**",
            ops: vec![],
        },
        Fixture {
            name: "conceal-away-from-caret",
            options: default_opts(),
            text: "plain\n# **word**\n> quote `code`\n",
            ops: vec![Op::Selection(0, 0)],
        },
        Fixture {
            name: "emoji-astral-mid-marker",
            options: default_opts(),
            text: "**a😀b** tail\n",
            ops: vec![Op::Edit(7, 7, "😀"), Op::Selection(14, 14)],
        },
        Fixture {
            name: "zwj-sequence",
            options: default_opts(),
            text: "**\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}** x\n",
            ops: vec![Op::Selection(0, 0)],
        },
        Fixture {
            name: "cjk-heading-and-body",
            options: default_opts(),
            text: "# 日本語の見出し\n本文です **強調** あり\n",
            ops: vec![Op::Selection(0, 0)],
        },
        Fixture {
            name: "nested-emphasis-triple",
            options: default_opts(),
            text: "***x*** and **_y_**\n",
            ops: vec![Op::Selection(20, 20)],
        },
        Fixture {
            name: "multi-backtick-code-span",
            options: default_opts(),
            text: "`` a`b `` end\n",
            ops: vec![Op::Selection(13, 13)],
        },
        Fixture {
            name: "empty-document",
            options: default_opts(),
            text: "",
            ops: vec![],
        },
        Fixture {
            name: "grow-from-empty",
            options: default_opts(),
            text: "",
            ops: vec![Op::Edit(0, 0, "# hi"), Op::Edit(4, 4, " **b**")],
        },
        Fixture {
            name: "no-trailing-newline",
            options: default_opts(),
            text: "**strong** end",
            ops: vec![],
        },
        Fixture {
            name: "block-split-typing-fence-above",
            options: default_opts(),
            text: "\nplain paragraph\n",
            ops: vec![Op::Edit(0, 0, "```")],
        },
        Fixture {
            name: "block-merge-delete-blank-line",
            options: default_opts(),
            text: "# one\n\ntwo\n",
            ops: vec![Op::Edit(6, 7, "")],
        },
        Fixture {
            name: "selection-spanning-lines",
            options: default_opts(),
            text: "# one\n**two**\n*three*\nplain\n",
            ops: vec![Op::Selection(0, 0), Op::Selection(8, 16)],
        },
        Fixture {
            name: "selection-reversed-normalizes",
            options: default_opts(),
            text: "# one\n**two**\n",
            ops: vec![Op::Selection(9, 2)],
        },
        Fixture {
            name: "task-list-elements",
            options: default_opts(),
            text: "- [ ] todo\n- [x] done\n- plain\n",
            ops: vec![Op::Selection(29, 29)],
        },
        Fixture {
            name: "links-and-images",
            options: default_opts(),
            text: "See [docs](https://e.com \"t\") and ![alt](img.png)\nnext\n",
            ops: vec![Op::Selection(52, 52)],
        },
        Fixture {
            name: "wikilinks",
            options: default_opts(),
            text: "[[Note]] and [[target|alias]]\ntail\n",
            ops: vec![Op::Selection(35, 35)],
        },
        Fixture {
            // Obsidian `![[file.png]]` attachment embeds: parse as Image
            // elements whose src is the bare target (plain + piped forms), with
            // `![[`/`]]` concealed off-caret. Caret parked on the tail line.
            name: "wiki-image-embed",
            options: default_opts(),
            text: "![[file.png]]\n![[photo.png|caption]]\ntail\n",
            ops: vec![Op::Selection(40, 40)],
        },
        Fixture {
            // Both image syntaxes in one document, so a replaying adapter has
            // to discriminate rather than pattern-match the `![[` opener: the
            // second line is ONE CommonMark image whose alt text starts with
            // `[`, and its `wiki` flag must be false while the first line's is
            // true. Their srcs are byte-identical, so only the flag tells them
            // apart — and it decides percent-decoding and resizability.
            name: "wiki-vs-commonmark-image",
            options: default_opts(),
            text: "![[a%20b.png]]\n![[bracketed] alt](a%20b.png)\ntail\n",
            ops: vec![Op::Selection(48, 48)],
        },
        Fixture {
            // Reference links resolve their dest to the URL inside the matching
            // definition below them (aux ⊄ element); each definition line
            // conceals off-caret like a thematic break and reveals raw when the
            // caret enters it. Second op parks the caret on the `[ref]:` line,
            // revealing that def while `[shortcut]:` stays concealed.
            name: "reference-links",
            options: default_opts(),
            text: "See [text][ref] and [shortcut].\n\n[ref]: https://example.com\n[shortcut]: https://ex.org\n",
            ops: vec![Op::Selection(2, 2), Op::Selection(40, 40)],
        },
        Fixture {
            // Empty piped alias: the mid-typing state that makes pulldown
            // re-emit the paragraph tail inside the still-open link (the
            // reversed-marker regression). The node conceals whole.
            name: "wikilink-empty-alias",
            options: default_opts(),
            text: "[[a|]] tail\nx\n",
            ops: vec![Op::Selection(13, 13)],
        },
        Fixture {
            name: "fence-and-quote",
            options: default_opts(),
            text: "> quote **b**\n\n```rust\nlet x = 1;\n```\n",
            ops: vec![Op::Selection(0, 0)],
        },
        Fixture {
            name: "table-block",
            options: default_opts(),
            text: "| a | b |\n|---|---|\n| **c** | d |\ntail\n",
            ops: vec![Op::Selection(38, 38)],
        },
        Fixture {
            name: "reader-mode-conceals-all",
            options: EngineOptions {
                reveal: RevealMode::None,
                ..Default::default()
            },
            text: "# h **b**\n- [ ] t\n",
            ops: vec![Op::Selection(3, 3)],
        },
        Fixture {
            name: "extensions-disabled",
            options: EngineOptions {
                extensions: Extensions::empty(),
                ..Default::default()
            },
            text: "~~not struck~~ and\n- [ ] not a task\n| a |\n|---|\n",
            ops: vec![],
        },
        Fixture {
            name: "setext-and-thematic-break",
            options: default_opts(),
            text: "Title\n=====\n\n---\n\ntext\n",
            ops: vec![Op::Selection(22, 22)],
        },
        Fixture {
            name: "edit-inside-marker-splits-strong",
            options: default_opts(),
            text: "**bold** text\n",
            ops: vec![Op::Edit(1, 1, "x")],
        },
        // -------------------------------------------- element-reveal-*
        Fixture {
            name: "element-reveal-inline-caret-inside",
            options: element_opts(),
            text: "plain **bold** text\n",
            ops: vec![Op::Selection(9, 9)],
        },
        Fixture {
            name: "element-reveal-inline-caret-outside",
            options: element_opts(),
            text: "plain **bold** text\n",
            ops: vec![Op::Selection(1, 1)],
        },
        Fixture {
            name: "element-reveal-boundary-start",
            options: element_opts(),
            text: "plain **bold** text\n",
            ops: vec![Op::Selection(6, 6)],
        },
        Fixture {
            name: "element-reveal-boundary-end",
            options: element_opts(),
            // Caret at owner.end and simultaneously EOF: doc ends exactly
            // at the closing `**` (no trailing text).
            text: "plain **bold**",
            ops: vec![Op::Selection(14, 14)],
        },
        Fixture {
            name: "element-reveal-adjacent-elements",
            options: element_opts(),
            text: "**a**_b_ tail\n",
            ops: vec![Op::Selection(5, 5)],
        },
        Fixture {
            name: "element-reveal-block-markers",
            options: element_opts(),
            text: "# heading\n> quote\n- [ ] task\n",
            ops: vec![Op::Selection(5, 5)],
        },
        Fixture {
            name: "element-reveal-selection-span",
            options: element_opts(),
            text: "**a** *b* ~~c~~ tail\n",
            ops: vec![Op::Selection(2, 12)],
        },
        Fixture {
            name: "element-reveal-selection-endpoint",
            options: element_opts(),
            text: "**a** *b* ~~c~~ tail\n",
            ops: vec![Op::Selection(5, 9)],
        },
        Fixture {
            name: "element-reveal-nested",
            options: element_opts(),
            text: "# **word** extra\n",
            ops: vec![Op::Selection(12, 12)],
        },
        Fixture {
            name: "element-reveal-multiline-span",
            options: element_opts(),
            text: "**a\nb** tail\nother\n",
            ops: vec![Op::Selection(2, 2)],
        },
    ]
}

// ------------------------------------------------------------- JSON writer

fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn kind_names(k: Kind) -> Vec<&'static str> {
    const NAMES: &[(Kind, &str)] = &[
        (Kind::BODY, "body"),
        (Kind::HEADING1, "heading1"),
        (Kind::HEADING2, "heading2"),
        (Kind::HEADING3, "heading3"),
        (Kind::HEADING4, "heading4"),
        (Kind::HEADING5, "heading5"),
        (Kind::HEADING6, "heading6"),
        (Kind::STRONG, "strong"),
        (Kind::EMPHASIS, "emphasis"),
        (Kind::STRIKETHROUGH, "strikethrough"),
        (Kind::CODE_SPAN, "code_span"),
        (Kind::CODE_BLOCK, "code_block"),
        (Kind::FENCE_INFO, "fence_info"),
        (Kind::BLOCKQUOTE, "blockquote"),
        (Kind::LIST_BULLET, "list_bullet"),
        (Kind::LIST_ORDINAL, "list_ordinal"),
        (Kind::TASK_MARKER, "task_marker"),
        (Kind::LINK, "link"),
        (Kind::IMAGE, "image"),
        (Kind::TABLE, "table"),
        (Kind::THEMATIC_BREAK, "thematic_break"),
        (Kind::MARKER, "marker"),
        (Kind::HTML_RAW, "html_raw"),
    ];
    NAMES
        .iter()
        .filter(|(bit, _)| k.contains(*bit))
        .map(|(_, n)| *n)
        .collect()
}

/// `{"start":s,"end":e,"utf16Start":a,"utf16End":b}` — every range in the
/// fixture carries both coordinate systems.
fn range_obj(e: &Engine, r: ByteRange, out: &mut String) {
    let _ = write!(
        out,
        "{{\"start\":{},\"end\":{},\"utf16Start\":{},\"utf16End\":{}}}",
        r.start,
        r.end,
        e.byte_to_utf16(r.start),
        e.byte_to_utf16(r.end)
    );
}

fn patch_json(e: &Engine, p: &Patch, out: &mut String) {
    out.push_str("{\"dirty\":[");
    for (i, r) in p.dirty.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        range_obj(e, *r, out);
    }
    out.push_str("]}");
}

fn export(f: &Fixture) -> String {
    let mut e = Engine::new(f.text, f.options);
    let mut patches_json: Vec<String> = Vec::new();

    for op in &f.ops {
        let patch = match *op {
            Op::Edit(s, t, ins) => e.apply_edit(ByteRange::new(s, t), ins),
            Op::Selection(s, t) => e.set_selection(ByteRange::new(s, t)),
        }
        .expect("fixture ops must be valid");
        let mut pj = String::new();
        patch_json(&e, &patch, &mut pj);
        patches_json.push(pj);
    }

    let mut out = String::new();
    out.push_str("{\n  \"schema\": 1,\n  \"name\": ");
    esc(f.name, &mut out);

    // Options.
    out.push_str(",\n  \"options\": {\"extensions\": [");
    let mut first = true;
    for (bit, name) in [
        (Extensions::TABLES, "tables"),
        (Extensions::TASK_LISTS, "task_lists"),
        (Extensions::STRIKETHROUGH, "strikethrough"),
        (Extensions::WIKILINKS, "wikilinks"),
    ] {
        if f.options.extensions.contains(bit) {
            if !first {
                out.push(',');
            }
            first = false;
            esc(name, &mut out);
        }
    }
    out.push_str("], \"reveal\": ");
    esc(
        match f.options.reveal {
            RevealMode::None => "none",
            RevealMode::Line => "line",
            RevealMode::Block => "block",
            RevealMode::Element => "element",
        },
        &mut out,
    );
    out.push_str("},\n  \"text\": ");
    esc(f.text, &mut out);

    // Ops.
    out.push_str(",\n  \"ops\": [");
    for (i, op) in f.ops.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    ");
        match *op {
            Op::Edit(s, t, ins) => {
                let _ = write!(out, "{{\"type\":\"edit\",\"start\":{s},\"end\":{t},\"insert\":");
                esc(ins, &mut out);
                out.push('}');
            }
            Op::Selection(s, t) => {
                let _ = write!(out, "{{\"type\":\"selection\",\"start\":{s},\"end\":{t}}}");
            }
        }
    }
    out.push_str(if f.ops.is_empty() { "]" } else { "\n  ]" });

    // Expectations: patches per op, then final segments and elements.
    out.push_str(",\n  \"expect\": {\n    \"patches\": [");
    for (i, pj) in patches_json.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n      ");
        out.push_str(pj);
    }
    out.push_str(if patches_json.is_empty() { "]" } else { "\n    ]" });

    out.push_str(",\n    \"segments\": [");
    let segs = e.segments_in(ByteRange::new(0, e.len_bytes())).to_vec();
    for (i, s) in segs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n      {\"range\":");
        range_obj(&e, s.range, &mut out);
        out.push_str(",\"kinds\":[");
        for (j, n) in kind_names(s.kinds).iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            esc(n, &mut out);
        }
        let _ = write!(
            out,
            "],\"concealed\":{}}}",
            if s.concealed { "true" } else { "false" }
        );
    }
    out.push_str(if segs.is_empty() { "]" } else { "\n    ]" });

    out.push_str(",\n    \"elements\": [");
    let els = e.elements_in(ByteRange::new(0, e.len_bytes())).to_vec();
    for (i, el) in els.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "\n      {{\"id\":{},\"range\":", el.id);
        range_obj(&e, el.range, &mut out);
        match el.kind {
            ElementKind::Task { checked } => {
                let _ = write!(
                    out,
                    ",\"kind\":\"task\",\"checked\":{}",
                    if checked { "true" } else { "false" }
                );
            }
            ElementKind::Link { dest } => {
                out.push_str(",\"kind\":\"link\",\"dest\":");
                range_obj(&e, dest, &mut out);
            }
            ElementKind::Image { src, wiki } => {
                out.push_str(",\"kind\":\"image\",\"src\":");
                range_obj(&e, src, &mut out);
                let _ = write!(out, ",\"wiki\":{}", if wiki { "true" } else { "false" });
            }
            ElementKind::Fence { info } => {
                out.push_str(",\"kind\":\"fence\",\"info\":");
                range_obj(&e, info, &mut out);
            }
            ElementKind::WikiLink { target } => {
                out.push_str(",\"kind\":\"wikilink\",\"target\":");
                range_obj(&e, target, &mut out);
            }
            ElementKind::Heading { level, text } => {
                let _ = write!(out, ",\"kind\":\"heading\",\"level\":{level}");
                out.push_str(",\"text\":");
                range_obj(&e, text, &mut out);
            }
        }
        out.push('}');
    }
    out.push_str(if els.is_empty() { "]" } else { "\n    ]" });

    // Final text and lengths for adapter-side desync checks.
    out.push_str(",\n    \"text\": ");
    esc(e.text(), &mut out);
    let _ = write!(
        out,
        ",\n    \"lenBytes\": {},\n    \"lenUtf16\": {}\n  }}\n}}\n",
        e.len_bytes(),
        e.len_utf16()
    );
    out
}

fn main() {
    // core/ is the crate root; fixtures/ lives at the repo root next to it.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    let all = fixtures();
    for f in &all {
        let json = export(f);
        let path = dir.join(format!("{}.json", f.name));
        std::fs::write(&path, json).expect("write fixture");
        println!("wrote {}", path.display());
    }
    println!("{} fixtures", all.len());
}
