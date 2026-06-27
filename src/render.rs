//! Pure rendering: parsed content in -> text / menu structures out.
//!
//! No sockets, no gopher protocol bytes, no daemon-specific formatting — this is
//! the testable core. Text pages come out as plain `String`s; menus come out as
//! a daemon-agnostic [`Vec<Entry>`]. Turning entries into a specific daemon's
//! index format (geomyidae `.gph`) is `gopher_core::render_menu_index`, not here.
//!
//! Selectors are the gopher selectors as served from the tree root, i.e. the
//! on-disk paths the publisher writes (`/index.gph`, `/posts/<slug>.txt`, ...).

use crate::content::Post;
use crate::markdown;

// The menu model + serializer now live in the shared `gopher-core` crate. We
// re-export the bits the page builders and `main` use so call sites stay
// `render::Entry` / `render::link` / etc.
pub use gopher_core::{info, link, Entry, ItemKind};

/// Stamp local links (those with no explicit host) with the tree's own
/// host/port, so generated `.gph` lines advertise a concrete address rather than
/// relying on the daemon's placeholder substitution (`--host`/`--port`). Info
/// lines and cross-server links (which already carry a host) are left untouched.
///
/// Blog-specific: gopher-cta leaves local links as placeholder tokens, so this
/// stays in the consumer (built on core's [`Entry::with_host`]).
pub fn with_host(entries: Vec<Entry>, host: &str, port: u16) -> Vec<Entry> {
    entries
        .into_iter()
        .map(|e| {
            if matches!(
                e,
                Entry::Link {
                    host: None,
                    port: None,
                    ..
                }
            ) {
                e.with_host(host, port)
            } else {
                e
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Blog page builders: menus (Vec<Entry>) and text pages (String).
// ---------------------------------------------------------------------------

/// Width of the `=`/`-` rules framing a post page.
const SEP: usize = 64;
/// Decorative banner rule for menus.
const BANNER: &str = "===============================================";

/// A tag/series facet: its slug (selector stem), display name, and the indices
/// (into the source post slice) of the posts carrying it, newest-first.
#[derive(Debug, Clone, PartialEq)]
pub struct Facet {
    pub slug: String,
    pub display: String,
    pub posts: Vec<usize>,
}

/// Selector/filename-safe stem for a tag or series name: lowercase ASCII
/// alphanumerics, every other run collapsed to a single `-`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("untagged");
    }
    out
}

/// A standard menu header: banner, subtitle, blank line.
fn header(subtitle: &str) -> Vec<Entry> {
    vec![
        info(BANNER),
        info(format!("  {subtitle}")),
        info(BANNER),
        info(""),
    ]
}

/// The root menu: banner, blurb, the section links, and (optionally) the hub
/// back-link to the cta hole.
pub fn root_menu(cta_link: Option<(&str, u16)>) -> Vec<Entry> {
    let mut e = header("gopher-blog : debene.dev over Gopher");
    e.push(info(
        "A phlog: posts from debene.dev, served as plain text.",
    ));
    e.push(info(""));
    e.push(link(ItemKind::Menu, "Posts", "/posts"));
    e.push(link(ItemKind::Menu, "Tags", "/tags"));
    e.push(link(ItemKind::Menu, "Series", "/series"));
    e.push(link(ItemKind::Text, "About", "/about.txt"));
    if let Some((host, port)) = cta_link {
        e.push(info(""));
        e.push(link(ItemKind::Menu, "Live CTA trains (gopher-cta)", "/").with_host(host, port));
    }
    e
}

/// A one-line menu entry for a post: `YYYY-MM-DD  Title` -> `/posts/<slug>.txt`.
fn post_link(p: &Post) -> Entry {
    link(
        ItemKind::Text,
        format!("{}  {}", p.date, p.title),
        format!("/posts/{}.txt", p.slug),
    )
}

/// The posts index: every post, newest-first.
pub fn posts_index(posts: &[Post]) -> Vec<Entry> {
    let mut e = header("Posts (newest first)");
    for p in posts {
        e.push(post_link(p));
    }
    e.push(info(""));
    e.push(link(ItemKind::Menu, "Back to root", "/"));
    e
}

/// Group posts by a multi-valued field (tags or series) into sorted facets. The
/// per-facet post indices preserve the input order (newest-first).
fn facets(posts: &[Post], pick: impl Fn(&Post) -> &[String]) -> Vec<Facet> {
    let mut out: Vec<Facet> = Vec::new();
    for (i, p) in posts.iter().enumerate() {
        for name in pick(p) {
            let slug = slugify(name);
            match out.iter_mut().find(|f| f.slug == slug) {
                Some(f) => f.posts.push(i),
                None => out.push(Facet {
                    slug,
                    display: name.clone(),
                    posts: vec![i],
                }),
            }
        }
    }
    out.sort_by_key(|f| f.display.to_lowercase());
    out
}

/// Tag facets, sorted by display name.
pub fn tag_facets(posts: &[Post]) -> Vec<Facet> {
    facets(posts, |p| &p.tags)
}

/// Series facets, sorted by display name.
pub fn series_facets(posts: &[Post]) -> Vec<Facet> {
    facets(posts, |p| &p.series)
}

/// The tags index: each tag with its post count, linking to its menu file.
pub fn tags_index(posts: &[Post]) -> Vec<Entry> {
    let mut e = header("Tags");
    for f in tag_facets(posts) {
        e.push(link(
            ItemKind::Menu,
            format!("{} ({})", f.display, f.posts.len()),
            format!("/tags/{}.gph", f.slug),
        ));
    }
    e.push(info(""));
    e.push(link(ItemKind::Menu, "Back to root", "/"));
    e
}

/// The series index: each series with its post count, linking to its menu file.
pub fn series_index(posts: &[Post]) -> Vec<Entry> {
    let mut e = header("Series");
    for f in series_facets(posts) {
        e.push(link(
            ItemKind::Menu,
            format!("{} ({})", f.display, f.posts.len()),
            format!("/series/{}.gph", f.slug),
        ));
    }
    e.push(info(""));
    e.push(link(ItemKind::Menu, "Back to root", "/"));
    e
}

/// A tag's menu: the posts carrying it, newest-first.
pub fn tag_menu(posts: &[Post], f: &Facet) -> Vec<Entry> {
    let mut e = header(&format!("Tag: {}", f.display));
    for &i in &f.posts {
        e.push(post_link(&posts[i]));
    }
    e.push(info(""));
    e.push(link(ItemKind::Menu, "All tags", "/tags"));
    e.push(link(ItemKind::Menu, "Back to root", "/"));
    e
}

/// A series' menu: the posts in it, in series (chronological) order — oldest
/// first, so the series reads start to finish.
pub fn series_menu(posts: &[Post], f: &Facet) -> Vec<Entry> {
    let mut e = header(&format!("Series: {}", f.display));
    for &i in f.posts.iter().rev() {
        e.push(post_link(&posts[i]));
    }
    e.push(info(""));
    e.push(link(ItemKind::Menu, "All series", "/series"));
    e.push(link(ItemKind::Menu, "Back to root", "/"));
    e
}

/// A full post page: framed header, the rendered body, the Links footnote
/// footer (when any), and the web/nav trailer.
pub fn post_page(post: &Post) -> String {
    let rendered = markdown::render(&post.body, &post.slug);
    let bar = "=".repeat(SEP);
    let rule = "-".repeat(SEP);
    let mut out = String::new();

    out.push_str(&bar);
    out.push('\n');
    for l in markdown::wrap(&post.title, SEP - 2) {
        out.push_str("  ");
        out.push_str(&l);
        out.push('\n');
    }
    let meta = if post.tags.is_empty() {
        post.date.clone()
    } else {
        format!("{} · {}", post.date, post.tags.join(", "))
    };
    for l in markdown::wrap(&meta, SEP - 2) {
        out.push_str("  ");
        out.push_str(&l);
        out.push('\n');
    }
    out.push_str(&bar);
    out.push('\n');
    out.push('\n');

    if !rendered.body.is_empty() {
        out.push_str(&rendered.body);
        out.push('\n');
    }

    if !rendered.footnotes.is_empty() {
        out.push('\n');
        out.push_str(&rule);
        out.push('\n');
        out.push_str("Links\n");
        for f in &rendered.footnotes {
            match &f.label {
                Some(label) => out.push_str(&format!("  [{}] {}  ({label})\n", f.n, f.url)),
                None => out.push_str(&format!("  [{}] {}\n", f.n, f.url)),
            }
        }
        out.push_str(&rule);
        out.push('\n');
    }

    out.push_str(&format!(
        "Read on the web: https://debene.dev/posts/{}/\n",
        post.slug
    ));
    out.push_str("Back: /posts  ·  Root: /\n");
    out
}

/// The about page: render `content/about.md` if present, else a small stub. The
/// `markdown` is the raw about.md content (frontmatter already stripped).
pub fn about_page(markdown_src: Option<&str>) -> String {
    let bar = "=".repeat(SEP);
    let mut out = String::new();
    out.push_str(&bar);
    out.push('\n');
    out.push_str("  About\n");
    out.push_str(&bar);
    out.push('\n');
    out.push('\n');
    match markdown_src {
        Some(src) => {
            let r = markdown::render(src, "about");
            out.push_str(&r.body);
            out.push('\n');
            if !r.footnotes.is_empty() {
                let rule = "-".repeat(SEP);
                out.push('\n');
                out.push_str(&rule);
                out.push('\n');
                out.push_str("Links\n");
                for f in &r.footnotes {
                    match &f.label {
                        Some(label) => out.push_str(&format!("  [{}] {}  ({label})\n", f.n, f.url)),
                        None => out.push_str(&format!("  [{}] {}\n", f.n, f.url)),
                    }
                }
                out.push_str(&rule);
                out.push('\n');
            }
        }
        None => {
            out.push_str("debene.dev, served over gopher.\n\n");
            out.push_str("Posts are rendered from the same source as the web blog,\n");
            out.push_str("as plain text with footnoted links. Browse them under Posts.\n\n");
            out.push_str("Read on the web: https://debene.dev/\n");
        }
    }
    out.push_str("\nBack: /  ·  Web: https://debene.dev/\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Post;

    fn post(
        slug: &str,
        date: &str,
        title: &str,
        tags: &[&str],
        series: &[&str],
        body: &str,
    ) -> Post {
        Post {
            slug: slug.to_string(),
            title: title.to_string(),
            date: date.to_string(),
            sort_key: 0,
            draft: false,
            description: String::new(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            categories: Vec::new(),
            series: series.iter().map(|s| s.to_string()).collect(),
            body: body.to_string(),
        }
    }

    fn selectors(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Link { selector, .. } => Some(selector.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn root_menu_has_sections_and_cta_link() {
        let e = root_menu(Some(("gopher.debene.dev", 70)));
        let sels = selectors(&e);
        assert!(sels.contains(&"/posts"));
        assert!(sels.contains(&"/tags"));
        assert!(sels.contains(&"/series"));
        assert!(sels.contains(&"/about.txt"));
        // the cta link is a remote link carrying host/port
        assert!(e.iter().any(|x| matches!(
            x,
            Entry::Link { host: Some(h), port: Some(70), selector, .. }
                if h == "gopher.debene.dev" && selector == "/"
        )));
        // without a cta link, no remote entry
        let e2 = root_menu(None);
        assert!(!e2
            .iter()
            .any(|x| matches!(x, Entry::Link { host: Some(_), .. })));
    }

    #[test]
    fn posts_index_lists_newest_first_with_date_titles() {
        let posts = vec![
            post("b", "2026-06-23", "Newer", &[], &[], ""),
            post("a", "2025-01-15", "Older", &[], &[], ""),
        ];
        let e = posts_index(&posts);
        let displays: Vec<&str> = e
            .iter()
            .filter_map(|x| match x {
                Entry::Link { display, .. } => Some(display.as_str()),
                _ => None,
            })
            .collect();
        assert!(displays.contains(&"2026-06-23  Newer"));
        assert!(selectors(&e).contains(&"/posts/b.txt"));
        // input order preserved (caller sorts newest-first)
        let i_new = displays.iter().position(|d| d.contains("Newer")).unwrap();
        let i_old = displays.iter().position(|d| d.contains("Older")).unwrap();
        assert!(i_new < i_old);
    }

    #[test]
    fn tags_index_counts_and_links() {
        let posts = vec![
            post("a", "2026-01-01", "A", &["rust", "gopher"], &[], ""),
            post("b", "2026-01-02", "B", &["rust"], &[], ""),
        ];
        let e = tags_index(&posts);
        let displays: Vec<String> = e
            .iter()
            .filter_map(|x| match x {
                Entry::Link { display, .. } => Some(display.clone()),
                _ => None,
            })
            .collect();
        assert!(displays.iter().any(|d| d == "rust (2)"));
        assert!(displays.iter().any(|d| d == "gopher (1)"));
        assert!(selectors(&e).contains(&"/tags/rust.gph"));
    }

    #[test]
    fn slugify_handles_punctuation_and_case() {
        assert_eq!(slugify("AI/ML"), "ai-ml");
        assert_eq!(slugify(".NET"), "net");
        assert_eq!(slugify("Quiet Internet"), "quiet-internet");
        assert_eq!(slugify("ppc64le"), "ppc64le");
    }

    #[test]
    fn series_menu_is_oldest_first() {
        // posts slice is newest-first; series menu should reverse to oldest-first
        let posts = vec![
            post("new", "2026-06-01", "Part 3", &[], &["Saga"], ""),
            post("mid", "2026-03-01", "Part 2", &[], &["Saga"], ""),
            post("old", "2026-01-01", "Part 1", &[], &["Saga"], ""),
        ];
        let f = &series_facets(&posts)[0];
        let e = series_menu(&posts, f);
        let sels = selectors(&e);
        let i_old = sels.iter().position(|s| *s == "/posts/old.txt").unwrap();
        let i_new = sels.iter().position(|s| *s == "/posts/new.txt").unwrap();
        assert!(i_old < i_new, "series should read oldest-first");
    }

    #[test]
    fn post_page_has_header_body_and_footer() {
        let p = post(
            "gopher-cta-live-trains",
            "2026-06-23",
            "Live Trains on a Quiet Internet",
            &["gopher", "rust"],
            &[],
            "Hello there, see [the docs](https://example.org/d).\n",
        );
        let page = post_page(&p);
        assert!(page.contains(&"=".repeat(SEP)));
        assert!(page.contains("  Live Trains on a Quiet Internet"));
        assert!(page.contains("2026-06-23 · gopher, rust"));
        // body with inline footnote marker
        assert!(page.contains("the docs[1]"));
        // Links footer
        assert!(page.contains("Links\n"));
        assert!(page.contains("[1] https://example.org/d"));
        // web + nav trailer
        assert!(page.contains("Read on the web: https://debene.dev/posts/gopher-cta-live-trains/"));
        assert!(page.contains("Back: /posts  ·  Root: /"));
    }

    #[test]
    fn about_stub_when_absent() {
        let page = about_page(None);
        assert!(page.contains("About"));
        assert!(page.contains("https://debene.dev/"));
    }

    // Byte-golden: locks the serialized /posts index .gph through the
    // gopher-core serializer + host stamping. Guards the extraction (and any
    // future change) against silent drift in the on-the-wire format.
    #[test]
    fn posts_index_gph_byte_golden() {
        let posts = vec![
            post("a-slug", "2026-06-23", "First Post", &[], &[], ""),
            post("b-slug", "2026-01-02", "Second Post", &[], &[], ""),
        ];
        let gph = gopher_core::render_menu_index(&with_host(
            posts_index(&posts),
            "gopher.debene.dev",
            7071,
        ));
        let expected = format!(
            "{BANNER}\n  Posts (newest first)\n{BANNER}\n\n\
             [0|2026-06-23  First Post|/posts/a-slug.txt|gopher.debene.dev|7071]\n\
             [0|2026-01-02  Second Post|/posts/b-slug.txt|gopher.debene.dev|7071]\n\
             \n[1|Back to root|/|gopher.debene.dev|7071]\n"
        );
        assert_eq!(gph, expected);
    }
}
