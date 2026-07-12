use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn dump(label: &str, text: &str) {
    println!("\n=== {label} ===");
    println!("TEXT: {text:?}");
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, opts).into_offset_iter();
    for (ev, range) in parser {
        let slice = &text[range.clone()];
        let kind = match &ev {
            Event::Start(t) => format!("Start({})", tag_name(t)),
            Event::End(t) => format!("End({})", tagend_name(t)),
            Event::Text(s) => format!("Text({s:?})"),
            Event::Code(s) => format!("Code({s:?})"),
            Event::Html(s) => format!("Html({s:?})"),
            Event::InlineHtml(s) => format!("InlineHtml({s:?})"),
            Event::SoftBreak => "SoftBreak".into(),
            Event::HardBreak => "HardBreak".into(),
            Event::Rule => "Rule".into(),
            Event::TaskListMarker(b) => format!("TaskListMarker({b})"),
            Event::FootnoteReference(s) => format!("FootnoteReference({s:?})"),
            Event::InlineMath(s) => format!("InlineMath({s:?})"),
            Event::DisplayMath(s) => format!("DisplayMath({s:?})"),
        };
        println!("  {:>3}..{:<3} {:<32} slice={:?}", range.start, range.end, kind, slice);
    }
}

fn tag_name(t: &Tag) -> String {
    match t {
        Tag::Paragraph => "Paragraph".into(),
        Tag::Heading { level, .. } => format!("Heading{level}"),
        Tag::BlockQuote(_) => "BlockQuote".into(),
        Tag::CodeBlock(k) => format!("CodeBlock({k:?})"),
        Tag::List(n) => format!("List({n:?})"),
        Tag::Item => "Item".into(),
        Tag::Emphasis => "Emphasis".into(),
        Tag::Strong => "Strong".into(),
        Tag::Strikethrough => "Strikethrough".into(),
        Tag::Link { dest_url, title, .. } => format!("Link(dest={dest_url:?},title={title:?})"),
        Tag::Image { dest_url, .. } => format!("Image(dest={dest_url:?})"),
        Tag::Table(_) => "Table".into(),
        Tag::TableHead => "TableHead".into(),
        Tag::TableRow => "TableRow".into(),
        Tag::TableCell => "TableCell".into(),
        Tag::FootnoteDefinition(_) => "FootnoteDefinition".into(),
        Tag::HtmlBlock => "HtmlBlock".into(),
        Tag::MetadataBlock(_) => "MetadataBlock".into(),
        Tag::DefinitionList => "DefinitionList".into(),
        Tag::DefinitionListTitle => "DefinitionListTitle".into(),
        Tag::DefinitionListDefinition => "DefinitionListDefinition".into(),
        Tag::Superscript => "Superscript".into(),
        Tag::Subscript => "Subscript".into(),
    }
}

fn tagend_name(t: &TagEnd) -> String {
    format!("{t:?}")
}

fn main() {
    dump("atx heading strong", "# **word**");
    dump("heading levels", "## Two\n### Three");
    dump("setext heading", "Title\n=====\n\nSub\n---");
    dump("emphasis nested", "***x***");
    dump("strong emph mix", "**_x_**");
    dump("strikethrough", "~~gone~~");
    dump("code span", "a `code` b");
    dump("multi backtick", "`` a`b ``");
    dump("link", "[text](https://example.com)");
    dump("link title", "[text](https://example.com \"t\")");
    dump("image", "![alt](img.png)");
    dump("fenced code", "```rust\nfn x(){}\n```");
    dump("fenced no info", "```\ncode\n```");
    dump("indented code", "    indented\n    more");
    dump("blockquote", "> quoted\n> more");
    dump("bullet list", "- one\n- two");
    dump("ordinal list", "1. one\n2. two");
    dump("task list", "- [ ] todo\n- [x] done");
    dump("thematic break", "---");
    dump("table", "| a | b |\n|---|---|\n| 1 | 2 |");
    dump("inline html", "a <b>bold</b> c");
    dump("html block", "<div>\nhi\n</div>");
    dump("emoji mid", "a😀b **x**");
    dump("cjk", "日本語 **strong**");
    dump("empty", "");
    dump("no trailing nl", "abc");
    dump("nested list", "- a\n  - b");
    dump("autolink", "<https://example.com>");
    dump("escaped", "a \\* b");
}
