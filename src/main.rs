//! gopher-blog: render the debene.dev Hugo blog into a static gopher (phlog)
//! tree, then atomically publish it for an external daemon (geomyidae) to serve.
//!
//! One shot: discover -> render -> publish -> exit. No loop, no network, no async.

mod content;
mod markdown;
mod publish;
mod render;

use std::path::PathBuf;
use std::process;

/// Parsed command line.
struct Config {
    content: PathBuf,
    out: PathBuf,
    /// Host/port stamped into generated `.gph` link columns for this tree.
    host: String,
    port: u16,
    /// The hub back-link to the cta hole: `(host, port)` parsed from `--cta-link`.
    cta_link: Option<(String, u16)>,
    /// Snapshots to retain on GC.
    keep: usize,
}

const USAGE: &str = "\
gopher-blog -- render the debene.dev blog into a gopher tree

USAGE:
    gopher-blog --content <dir> --out <dir>
                [--host gopher.debene.dev] [--port 70]
                [--cta-link gopher://gopher.debene.dev:70]
                [--keep 3]

OPTIONS:
    --content <dir>   Hugo content root (contains posts/*/index.md). Required.
    --out <dir>       Output root; snapshots written as out-<ts>/, current flipped.
    --host <host>     Host advertised in generated .gph links. Default gopher.debene.dev.
    --port <port>     Port advertised in generated .gph links. Default 70.
    --cta-link <url>  gopher:// back-link to the cta hole in the root menu.
                      Default gopher://gopher.debene.dev:70. Pass \"none\" to omit.
    --keep <n>        Snapshots to retain on GC. Default 3.
    -h, --help        Show this help.";

fn main() {
    let cfg = match parse_args(std::env::args().skip(1)) {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(e) => {
            eprintln!("gopher-blog: {e}\n\n{USAGE}");
            process::exit(2);
        }
    };

    if let Err(e) = run(&cfg) {
        eprintln!("gopher-blog: {e}");
        process::exit(1);
    }
}

/// Render the tree and publish it. For now (commit 1) this publishes a minimal
/// root menu only — content discovery and page rendering land in later commits.
fn run(cfg: &Config) -> Result<(), String> {
    if !cfg.content.is_dir() {
        return Err(format!(
            "--content {} is not a directory",
            cfg.content.display()
        ));
    }

    // Discover posts (drafts skipped, newest-first).
    let posts = content::discover(&cfg.content).map_err(|e| format!("discover posts: {e}"))?;
    println!("discovered {} non-draft post(s)", posts.len());

    let files = build_tree(&posts, cfg);

    let snap = publish::publish(&cfg.out, &files, cfg.keep)
        .map_err(|e| format!("publish to {}: {e}", cfg.out.display()))?;
    println!(
        "published {} files: {} -> {}",
        files.len(),
        snap.display(),
        cfg.out.join("current").display()
    );
    Ok(())
}

/// Render the full gopher tree into a publishable file map.
fn build_tree(posts: &[content::Post], cfg: &Config) -> Vec<publish::TreeFile> {
    let mut files: Vec<publish::TreeFile> = Vec::new();

    // Local closure: serialize a menu, stamping the tree's own host/port.
    let menu = |entries: Vec<render::Entry>| -> Vec<u8> {
        publish::render_menu_index(&render::with_host(entries, &cfg.host, cfg.port)).into_bytes()
    };

    let cta = cfg.cta_link.as_ref().map(|(h, p)| (h.as_str(), *p));

    // Root + section indexes.
    files.push(("index.gph".into(), menu(render::root_menu(cta))));
    files.push(("posts/index.gph".into(), menu(render::posts_index(posts))));
    files.push(("tags/index.gph".into(), menu(render::tags_index(posts))));
    files.push(("series/index.gph".into(), menu(render::series_index(posts))));

    // About: render content/about.md if present, else a stub.
    let about_src = read_about(&cfg.content);
    files.push((
        "about.txt".into(),
        render::about_page(about_src.as_deref()).into_bytes(),
    ));

    // One text page per post.
    for p in posts {
        files.push((
            format!("posts/{}.txt", p.slug),
            render::post_page(p).into_bytes(),
        ));
    }

    // One menu file per tag / series facet.
    for f in render::tag_facets(posts) {
        files.push((
            format!("tags/{}.gph", f.slug),
            menu(render::tag_menu(posts, &f)),
        ));
    }
    for f in render::series_facets(posts) {
        files.push((
            format!("series/{}.gph", f.slug),
            menu(render::series_menu(posts, &f)),
        ));
    }

    files
}

/// Read `content/about.md`, stripping a leading YAML frontmatter block if any.
/// Returns `None` if the file is absent.
fn read_about(content: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(content.join("about.md")).ok()?;
    let body = match raw.strip_prefix("---\n") {
        Some(after) => after
            .split_once("\n---\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or(raw.clone()),
        None => raw,
    };
    Some(body)
}

/// Hand-rolled flag parser (no clap; keeps the dep list minimal). Returns
/// `Ok(None)` when `--help` was requested.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Config>, String> {
    let mut content: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut host = "gopher.debene.dev".to_string();
    let mut port: u16 = 70;
    let mut cta_link_raw = "gopher://gopher.debene.dev:70".to_string();
    let mut keep = publish::KEEP_SNAPSHOTS;

    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut next = |flag: &str| -> Result<String, String> {
            args.next()
                .ok_or_else(|| format!("flag {flag} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--content" => content = Some(PathBuf::from(next("--content")?)),
            "--out" => out = Some(PathBuf::from(next("--out")?)),
            "--host" => host = next("--host")?,
            "--port" => {
                let v = next("--port")?;
                port = v.parse().map_err(|_| format!("invalid --port: {v}"))?;
            }
            "--cta-link" => cta_link_raw = next("--cta-link")?,
            "--keep" => {
                let v = next("--keep")?;
                keep = v.parse().map_err(|_| format!("invalid --keep: {v}"))?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let content = content.ok_or("--content is required")?;
    let out = out.ok_or("--out is required")?;
    let cta_link = parse_cta_link(&cta_link_raw)?;

    Ok(Some(Config {
        content,
        out,
        host,
        port,
        cta_link,
        keep,
    }))
}

/// Parse `--cta-link`: a `gopher://host[:port]` URL into `(host, port)`. The
/// literal `none` (or empty) disables the back-link. Port defaults to 70.
fn parse_cta_link(raw: &str) -> Result<Option<(String, u16)>, String> {
    if raw.is_empty() || raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let rest = raw
        .strip_prefix("gopher://")
        .ok_or_else(|| format!("--cta-link must start with gopher:// (got {raw})"))?;
    // Strip any trailing path; host[:port] is the authority.
    let authority = rest.split('/').next().unwrap_or(rest);
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse()
                .map_err(|_| format!("invalid port in --cta-link: {p}"))?;
            (h.to_string(), port)
        }
        None => (authority.to_string(), 70u16),
    };
    if host.is_empty() {
        return Err(format!("--cta-link has no host: {raw}"));
    }
    Ok(Some((host, port)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> impl Iterator<Item = String> {
        v.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn parses_required_and_defaults() {
        let cfg = parse_args(args(&["--content", "c", "--out", "o"]))
            .unwrap()
            .unwrap();
        assert_eq!(cfg.content, PathBuf::from("c"));
        assert_eq!(cfg.out, PathBuf::from("o"));
        assert_eq!(cfg.host, "gopher.debene.dev");
        assert_eq!(cfg.port, 70);
        assert_eq!(cfg.keep, 3);
        assert_eq!(cfg.cta_link, Some(("gopher.debene.dev".to_string(), 70)));
    }

    #[test]
    fn missing_required_errors() {
        assert!(parse_args(args(&["--content", "c"])).is_err());
    }

    #[test]
    fn help_returns_none() {
        assert!(parse_args(args(&["--help"])).unwrap().is_none());
    }

    #[test]
    fn cta_link_variants() {
        assert_eq!(
            parse_cta_link("gopher://example.org:7070").unwrap(),
            Some(("example.org".to_string(), 7070))
        );
        // default port
        assert_eq!(
            parse_cta_link("gopher://example.org").unwrap(),
            Some(("example.org".to_string(), 70))
        );
        // trailing path ignored
        assert_eq!(
            parse_cta_link("gopher://example.org:70/").unwrap(),
            Some(("example.org".to_string(), 70))
        );
        // disabled
        assert_eq!(parse_cta_link("none").unwrap(), None);
        assert_eq!(parse_cta_link("").unwrap(), None);
        // bad scheme
        assert!(parse_cta_link("http://x").is_err());
    }
}
