//! Markdown -> gopher-text. Pure: a markdown string (plus the post slug, needed
//! to resolve relative URLs) goes in; a reflowed gopher-text body and a collected
//! footnote table come out. The full post page (header + body + Links footer) is
//! assembled in [`crate::render`].
//!
//! This covers what the debene.dev posts actually use, not all of CommonMark:
//! headings, paragraphs (70-col word wrap), code fences (verbatim), inline links
//! (footnoted), images (`[img: alt]` + footnote), lists, blockquotes, and tables
//! (aligned if they fit, else a "see web" fallback).

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Body word-wrap column.
const WRAP: usize = 70;
/// Width of the thin rule that brackets a code block / closes the body.
const RULE: usize = 64;

/// One footnote: a link or image reference collected from the body. `label` is
/// `Some("img: <alt>")` for images (and `Some("table")` for the table fallback),
/// `None` for plain links. Numbering is per page, links and images sharing one
/// sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct Footnote {
    pub n: usize,
    pub url: String,
    pub label: Option<String>,
}

/// The rendered body plus its footnote table.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub body: String,
    pub footnotes: Vec<Footnote>,
}

/// Render markdown to a gopher-text body + footnote table. `slug` resolves
/// relative link/image URLs to their canonical `https://debene.dev/...` form.
pub fn render(markdown: &str, slug: &str) -> Rendered {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, opts);

    let mut r = Md::new(slug);
    for ev in parser {
        r.event(ev);
    }
    r.finish()
}

/// Resolve a markdown URL to an absolute one. Absolute URLs (and `mailto:`,
/// protocol-relative) pass through; site-absolute (`/x`) and relative (`x`,
/// `images/y`) resolve against the post's canonical web location.
fn resolve_url(url: &str, slug: &str) -> String {
    let u = url.trim();
    if u.starts_with("http://")
        || u.starts_with("https://")
        || u.starts_with("gopher://")
        || u.starts_with("mailto:")
        || u.starts_with("//")
    {
        u.to_string()
    } else if let Some(rest) = u.strip_prefix('/') {
        format!("https://debene.dev/{rest}")
    } else {
        format!("https://debene.dev/posts/{slug}/{u}")
    }
}

/// Greedy word-wrap `text` into `out`, with `first` prefixing the first line and
/// `cont` every following line. `width` caps the *total* line length (prefix
/// included). A single word longer than the budget is emitted alone, overflowing
/// — the intended exception for bare URLs.
fn wrap_into(text: &str, width: usize, first: &str, cont: &str, out: &mut Vec<String>) {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return;
    }
    let mut prefix = first;
    let mut cur = String::new();
    for w in words {
        if cur.is_empty() {
            cur.push_str(w);
        } else if prefix.chars().count() + cur.chars().count() + 1 + w.chars().count() <= width {
            cur.push(' ');
            cur.push_str(w);
        } else {
            out.push(format!("{prefix}{cur}"));
            prefix = cont;
            cur = w.to_string();
        }
    }
    out.push(format!("{prefix}{cur}"));
}

/// Word-wrap `text` to `width`, returning the lines (no prefixes). Public so the
/// page builder can wrap titles / meta lines with the same greedy algorithm.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    wrap_into(text, width, "", "", &mut out);
    out
}

/// A list nesting frame: `Some(next)` is an ordered list (with the next item
/// number), `None` a bullet list.
struct ListFrame {
    ordered: Option<u64>,
}

/// Accumulating table: header row + body rows of plain-text cells.
#[derive(Default)]
struct TableAcc {
    rows: Vec<Vec<String>>,
    cur: Vec<String>,
}

struct Md<'a> {
    slug: &'a str,
    lines: Vec<String>,
    footnotes: Vec<Footnote>,
    n: usize,
    /// Inline text accumulator for the current leaf block.
    inline: String,
    /// Pending link destination (resolved); marker appended at End(Link).
    link: Option<String>,
    /// Pending image `(resolved dest, alt accumulator)`.
    image: Option<(String, String)>,
    /// Open list frames (innermost last).
    lists: Vec<ListFrame>,
    /// Per-open-item `(first-line prefix, continuation prefix, lead-emitted?)`.
    items: Vec<(String, String, bool)>,
    /// Blockquote nesting depth.
    quote: usize,
    /// Current heading level, set between Start/End(Heading).
    heading: Option<HeadingLevel>,
    in_code: bool,
    code: String,
    table: Option<TableAcc>,
}

impl<'a> Md<'a> {
    fn new(slug: &'a str) -> Self {
        Md {
            slug,
            lines: Vec::new(),
            footnotes: Vec::new(),
            n: 0,
            inline: String::new(),
            link: None,
            image: None,
            lists: Vec::new(),
            items: Vec::new(),
            quote: 0,
            heading: None,
            in_code: false,
            code: String::new(),
            table: None,
        }
    }

    fn finish(mut self) -> Rendered {
        self.flush_inline();
        // Trim leading/trailing blank lines.
        while self.lines.first().is_some_and(|l| l.is_empty()) {
            self.lines.remove(0);
        }
        while self.lines.last().is_some_and(|l| l.is_empty()) {
            self.lines.pop();
        }
        Rendered {
            body: self.lines.join("\n"),
            footnotes: self.footnotes,
        }
    }

    fn container_depth(&self) -> usize {
        self.lists.len() + self.quote
    }

    /// Ensure exactly one blank line separates top-level blocks.
    fn block_sep(&mut self) {
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }

    fn next_n(&mut self) -> usize {
        self.n += 1;
        self.n
    }

    /// Prefixes + width for the current flush context (list item > blockquote >
    /// top level).
    fn current_prefixes(&self) -> (String, String, usize) {
        if let Some((first, cont, lead_done)) = self.items.last() {
            let f = if *lead_done {
                cont.clone()
            } else {
                first.clone()
            };
            (f, cont.clone(), WRAP)
        } else if self.quote > 0 {
            let p = "> ".repeat(self.quote);
            (p.clone(), p, WRAP)
        } else {
            (String::new(), String::new(), WRAP)
        }
    }

    /// Flush the inline accumulator as a wrapped block in the current context.
    fn flush_inline(&mut self) {
        if self.inline.trim().is_empty() {
            self.inline.clear();
            return;
        }
        let text = std::mem::take(&mut self.inline);
        let (first, cont, width) = self.current_prefixes();
        wrap_into(&text, width, &first, &cont, &mut self.lines);
        if let Some(item) = self.items.last_mut() {
            item.2 = true; // lead emitted; further paragraphs use the cont prefix
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if self.in_code {
                    self.code.push_str(&t);
                } else if let Some((_, alt)) = self.image.as_mut() {
                    alt.push_str(&t);
                } else {
                    self.inline.push_str(&t);
                }
            }
            Event::Code(c) => {
                // inline code: keep the literal text (no backticks)
                if let Some((_, alt)) = self.image.as_mut() {
                    alt.push_str(&c);
                } else {
                    self.inline.push_str(&c);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if !self.in_code {
                    self.inline.push(' ');
                }
            }
            Event::Rule => {
                // A prose thematic break. Deliberately distinct from the code
                // bracket rule (`-` * RULE) so the two never read alike.
                self.flush_inline();
                self.block_sep();
                self.lines.push("* * *".to_string());
            }
            // Raw HTML and footnote refs are dropped (not used by these posts).
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if self.container_depth() == 0 {
                    self.block_sep();
                }
            }
            Tag::Heading { level, .. } => {
                self.block_sep();
                self.heading = Some(level);
            }
            Tag::CodeBlock(_) => {
                self.flush_inline();
                self.in_code = true;
                self.code.clear();
            }
            Tag::List(start) => {
                // Flush any lead text of the enclosing item before nesting.
                self.flush_inline();
                if self.container_depth() == 0 {
                    self.block_sep();
                }
                self.lists.push(ListFrame { ordered: start });
            }
            Tag::Item => {
                let depth = self.lists.len().saturating_sub(1);
                let indent = "  ".repeat(depth);
                let marker = match self.lists.last_mut() {
                    Some(ListFrame { ordered: Some(num) }) => {
                        let m = format!("{num}. ");
                        *num += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                let first = format!("{indent}{marker}");
                let cont = format!("{indent}{}", " ".repeat(marker.chars().count()));
                self.items.push((first, cont, false));
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                if self.container_depth() == 0 {
                    self.block_sep();
                }
                self.quote += 1;
            }
            Tag::Link { dest_url, .. } => {
                self.link = Some(resolve_url(&dest_url, self.slug));
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some((resolve_url(&dest_url, self.slug), String::new()));
            }
            Tag::Table(_) => {
                self.flush_inline();
                self.block_sep();
                self.table = Some(TableAcc::default());
            }
            Tag::TableCell => {
                self.inline.clear();
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_inline(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::CodeBlock => self.end_code(),
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush_inline();
                self.items.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::Link => {
                if let Some(url) = self.link.take() {
                    let n = self.next_n();
                    self.inline.push_str(&format!("[{n}]"));
                    self.footnotes.push(Footnote {
                        n,
                        url,
                        label: None,
                    });
                }
            }
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    let n = self.next_n();
                    let alt = alt.trim();
                    self.inline.push_str(&format!("[img: {alt}]"));
                    self.footnotes.push(Footnote {
                        n,
                        url,
                        label: Some(format!("img: {alt}")),
                    });
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline).trim().to_string();
                if let Some(t) = self.table.as_mut() {
                    t.cur.push(cell);
                }
            }
            TagEnd::TableRow | TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => self.end_table(),
            _ => {}
        }
    }

    fn end_heading(&mut self) {
        let level = self.heading.take().unwrap_or(HeadingLevel::H3);
        let text = std::mem::take(&mut self.inline);
        let mut block = Vec::new();
        wrap_into(text.trim(), WRAP, "", "", &mut block);
        if block.is_empty() {
            return;
        }
        let width = block.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let ruler = if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
            '='
        } else {
            '-'
        };
        for l in block {
            self.lines.push(l);
        }
        self.lines.push(ruler.to_string().repeat(width.min(WRAP)));
    }

    fn end_code(&mut self) {
        self.in_code = false;
        self.block_sep();
        self.lines.push("-".repeat(RULE));
        let code = std::mem::take(&mut self.code);
        // Emit verbatim; trailing newline shouldn't add a blank line.
        for line in code.strip_suffix('\n').unwrap_or(&code).split('\n') {
            self.lines.push(line.to_string());
        }
        self.lines.push("-".repeat(RULE));
    }

    fn end_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        if t.rows.is_empty() {
            return;
        }
        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; cols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        // Total monospace width: "| " + cells joined by " | " + " |".
        let total: usize = widths.iter().sum::<usize>() + 3 * cols + 1;
        if total > WRAP {
            // Doesn't fit: fall back to a "see web" pointer.
            let n = self.next_n();
            self.lines.push("[table — see web]".to_string());
            self.footnotes.push(Footnote {
                n,
                url: format!("https://debene.dev/posts/{}/", self.slug),
                label: Some("table".to_string()),
            });
            return;
        }
        let fmt_row = |row: &[String]| -> String {
            let mut s = String::from("|");
            for (i, w) in widths.iter().enumerate() {
                let cell = row.get(i).map(String::as_str).unwrap_or("");
                s.push_str(&format!(" {cell:w$} |"));
            }
            s
        };
        for (i, row) in t.rows.iter().enumerate() {
            self.lines.push(fmt_row(row));
            if i == 0 {
                // header separator
                let mut s = String::from("|");
                for w in &widths {
                    s.push_str(&format!(" {} |", "-".repeat(*w)));
                }
                self.lines.push(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(md: &str) -> String {
        render(md, "my-post").body
    }

    #[test]
    fn wraps_paragraphs_at_70() {
        let md = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
        let r = body(md);
        for line in r.lines() {
            assert!(line.chars().count() <= 70, "too long: {line:?}");
        }
        // greedy wrap actually splits into multiple lines
        assert!(r.lines().count() > 1);
    }

    #[test]
    fn heading_gets_underline() {
        let r = body("# Title Here");
        let mut lines = r.lines();
        assert_eq!(lines.next().unwrap(), "Title Here");
        assert_eq!(lines.next().unwrap(), "==========");
        // H3 uses a dashed rule
        let r3 = body("### Sub");
        assert!(r3.contains("Sub\n---"));
    }

    #[test]
    fn inline_link_footnoted() {
        let r = render("see [the docs](https://example.org/x) now", "my-post");
        assert!(r.body.contains("see the docs[1] now"));
        assert_eq!(r.footnotes.len(), 1);
        assert_eq!(r.footnotes[0].n, 1);
        assert_eq!(r.footnotes[0].url, "https://example.org/x");
        assert_eq!(r.footnotes[0].label, None);
    }

    #[test]
    fn relative_link_resolves_to_canonical() {
        let r = render("[pic](images/a.png)", "gopher-cta-live-trains");
        assert_eq!(
            r.footnotes[0].url,
            "https://debene.dev/posts/gopher-cta-live-trains/images/a.png"
        );
    }

    #[test]
    fn image_marker_and_resolved_url() {
        let r = render("![a hero shot](images/hero.png)", "gopher-cta-live-trains");
        assert!(r.body.contains("[img: a hero shot]"));
        assert_eq!(r.footnotes.len(), 1);
        assert_eq!(
            r.footnotes[0].url,
            "https://debene.dev/posts/gopher-cta-live-trains/images/hero.png"
        );
        assert_eq!(r.footnotes[0].label.as_deref(), Some("img: a hero shot"));
    }

    #[test]
    fn shared_footnote_sequence() {
        let r = render(
            "[one](https://a.test) ![two](b.png) [three](https://c.test)",
            "p",
        );
        let ns: Vec<usize> = r.footnotes.iter().map(|f| f.n).collect();
        assert_eq!(ns, vec![1, 2, 3]);
    }

    #[test]
    fn code_fence_passthrough_verbatim() {
        let md = "```\nlet x = some_very_long_identifier_that_exceeds_seventy_columns_for_sure_yes_indeed = 1;\n```";
        let r = body(md);
        assert!(r.contains(
            "let x = some_very_long_identifier_that_exceeds_seventy_columns_for_sure_yes_indeed = 1;"
        ));
        // bracketed by a thin rule
        assert!(r.starts_with(&"-".repeat(RULE)));
    }

    #[test]
    fn code_not_reflowed_preserves_indentation() {
        let md = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
        let r = body(md);
        assert!(r.contains("\n    println!(\"hi\");\n"));
    }

    #[test]
    fn bullet_and_ordered_lists() {
        let r = body("- first\n- second");
        assert!(r.contains("• first"));
        assert!(r.contains("• second"));
        let o = body("1. one\n2. two");
        assert!(o.contains("1. one"));
        assert!(o.contains("2. two"));
    }

    #[test]
    fn list_item_hanging_indent() {
        let long = "- ".to_string() + "word ".repeat(40).trim();
        let r = body(&long);
        let lines: Vec<&str> = r.lines().collect();
        assert!(lines[0].starts_with("• "));
        // continuation lines align under the text (two-space hang)
        assert!(lines[1].starts_with("  "));
        for l in &lines {
            assert!(l.chars().count() <= 70);
        }
    }

    #[test]
    fn blockquote_prefixed_and_wrapped() {
        let long = "> ".to_string() + &"quoted ".repeat(30);
        let r = body(&long);
        for line in r.lines() {
            assert!(line.starts_with("> "));
            assert!(line.chars().count() <= 70);
        }
    }

    #[test]
    fn small_table_renders_aligned() {
        let md = "| A | B |\n| - | - |\n| 1 | 2 |";
        let r = body(md);
        assert!(r.contains("| A | B |"));
        assert!(r.contains("| 1 | 2 |"));
        assert!(!r.contains("see web"));
    }

    // TEMP validation against the real debene-dev content; run with
    // DEBENE_CONTENT=<dir> cargo test real_posts -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_posts_obey_70_col_invariant() {
        let Ok(dir) = std::env::var("DEBENE_CONTENT") else {
            eprintln!("skipped: set DEBENE_CONTENT=<hugo content dir> to run");
            return;
        };
        let posts = std::path::Path::new(&dir).join("posts");
        let mut checked = 0;
        for e in std::fs::read_dir(&posts).unwrap() {
            let p = e.unwrap().path();
            let idx = p.join("index.md");
            if !idx.is_file() {
                continue;
            }
            let slug = p.file_name().unwrap().to_str().unwrap();
            let raw = std::fs::read_to_string(&idx).unwrap();
            let Some((_, b)) = raw.split_once("\n---\n") else {
                continue;
            };
            let r = render(b, slug);
            let rule = "-".repeat(RULE);
            let mut in_code = false;
            for line in r.body.lines() {
                if line == rule.as_str() {
                    in_code = !in_code;
                    continue;
                }
                let wide = line.chars().count() > 70;
                let bare_url = line.split_whitespace().count() == 1
                    && (line.contains("://") || line.contains("[img:"));
                assert!(
                    !wide || in_code || bare_url,
                    "{slug}: line >70 outside code/url: {line:?}"
                );
            }
            checked += 1;
        }
        println!("checked {checked} posts");
        assert!(checked > 0);
    }

    #[test]
    fn wide_table_falls_back_to_web() {
        let wide = "verylongheadercell".repeat(3);
        let md = format!("| {wide} | {wide} | {wide} |\n| - | - | - |\n| a | b | c |");
        let r = render(&md, "my-post");
        assert!(r.body.contains("[table — see web]"));
        assert_eq!(r.footnotes.len(), 1);
        assert_eq!(r.footnotes[0].url, "https://debene.dev/posts/my-post/");
    }
}
