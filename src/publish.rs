//! Publisher front end: write a static gopher tree that an external daemon
//! (geomyidae) serves. No sockets of our own — we render files.
//!
//! Publishing is **atomic**: each run renders into a fresh `out-<ts>/` snapshot
//! directory, then flips a `current` symlink to it with an atomic rename. The
//! daemon is pointed at `current/`, so a reader always sees a complete tree,
//! never a half-written one. Old snapshots are garbage-collected.
//!
//! Unlike gopher-cta this is a *one-shot* publisher: there is no loop. The caller
//! renders the whole tree into a file map and calls [`publish`] once.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::render;

/// Published snapshots to retain by default (besides whatever `current`
/// resolves to). Overridable per-run via the publisher's `keep` argument
/// (the `--keep` CLI flag).
pub const KEEP_SNAPSHOTS: usize = 3;

/// One file to write into a snapshot: `(path relative to the snapshot root,
/// bytes)`. The publisher creates intermediate directories as needed.
pub type TreeFile = (String, Vec<u8>);

/// Render the file map into a fresh `out-<ts>/`, then atomically flip `current`
/// and GC old snapshots (retaining `keep`). Returns the snapshot directory.
pub fn publish(out: &Path, files: &[TreeFile], keep: usize) -> io::Result<PathBuf> {
    fs::create_dir_all(out)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| io::Error::other(e.to_string()))?
        .as_nanos();
    let snap = out.join(format!("out-{ts}"));
    fs::create_dir_all(&snap)?;
    write_tree(&snap, files)?;
    flip_current(out, &snap)?;
    gc(out, &snap, keep)?;
    Ok(snap)
}

/// Write every file in the map into `dir`, creating parent directories as
/// needed. Blog-specific (gopher-cta's `write_tree` is hard-wired to its own
/// pages); the file map comes from [`crate::render`].
fn write_tree(dir: &Path, files: &[TreeFile]) -> io::Result<()> {
    for (rel, bytes) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    Ok(())
}

// EXTRACTION CANDIDATE -> gopher-core (extract once blog v1 renders; see DESIGN).
// Keep byte-identical to gopher-cta until then. Any edit here MUST also land in cta.
// NOTE: this `render_menu_index` is the *post-cross-link* shape — it reads
// `Entry::Link`'s `host`/`port`; a `None` host/port serializes byte-identically to
// today's cta output.

/// The single daemon-specific function: serialize a daemon-agnostic menu
/// ([`render::Entry`] list) into a geomyidae `.gph` index. **To target a
/// different daemon (e.g. Gophernicus `gophermap`), rewrite only this.**
///
/// Format (confirmed against geomyidae(8) and the phd implementation): a link is
/// `[<type>|<name>|<selector>|server|port]`; geomyidae substitutes the literal
/// tokens `server`/`port` with its own host/port at serve time, so the files
/// stay host/port-agnostic. A link carrying an explicit `host`/`port` (a
/// cross-server hub link) emits those concrete values instead. Any line not
/// starting with `[` is an info (i) line.
pub fn render_menu_index(entries: &[render::Entry]) -> String {
    use render::{Entry, ItemKind};
    let mut out = String::new();
    for e in entries {
        match e {
            Entry::Info(s) => {
                // Info text that happens to start with '[' would be mis-parsed as
                // a link; a leading space keeps it an info line.
                if s.starts_with('[') {
                    out.push(' ');
                }
                out.push_str(s);
                out.push('\n');
            }
            Entry::Link {
                kind,
                display,
                selector,
                host,
                port,
            } => {
                let t = match kind {
                    ItemKind::Text => '0',
                    ItemKind::Menu => '1',
                    ItemKind::Url => 'h',
                    ItemKind::Bin => '9',
                };
                // `None` -> the literal placeholder tokens geomyidae fills in.
                let server = host.as_deref().unwrap_or("server");
                let port_col = match port {
                    Some(p) => p.to_string(),
                    None => "port".to_string(),
                };
                out.push_str(&format!(
                    "[{t}|{}|{}|{}|{}]\n",
                    gph_escape(display),
                    gph_escape(selector),
                    gph_escape(server),
                    port_col,
                ));
            }
        }
    }
    out
}

/// Escape the `.gph` field separator `|` within a field (geomyidae uses `\|`).
fn gph_escape(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Atomically point `current` at `snap`: write a temp symlink then rename it over
/// `current`. rename(2) is atomic, so a reader resolves either the old target or
/// the new one — never a missing/half-built link. The link is relative
/// (`current -> out-<ts>`) so it stays valid under any mount path.
fn flip_current(out: &Path, snap: &Path) -> io::Result<()> {
    let target = snap.file_name().expect("snapshot dir has a file name");
    let tmp = out.join(format!(".current.tmp.{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp)?;
    fs::rename(&tmp, out.join("current"))
}

/// Remove old `out-*` snapshots, keeping the newest `keep` and never the one just
/// published. (gopher-cta hard-codes `KEEP_SNAPSHOTS`; here it is a parameter so
/// `--keep` can tune it — the one deliberate divergence from the cta copy.)
fn gc(out: &Path, keep_snap: &Path, keep: usize) -> io::Result<()> {
    let mut snaps: Vec<PathBuf> = fs::read_dir(out)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("out-"))
        })
        .collect();
    snaps.sort(); // nanosecond names sort chronologically; newest last
    let n = snaps.len();
    let keep_name = keep_snap.file_name();
    for (i, p) in snaps.iter().enumerate() {
        let is_recent = i + keep >= n;
        let is_current = p.file_name() == keep_name;
        if !is_recent && !is_current {
            let _ = fs::remove_dir_all(p);
        }
    }
    Ok(())
}
// END EXTRACTION CANDIDATE.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique temp dir for a test, removed on drop.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> TmpDir {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p =
                std::env::temp_dir().join(format!("gopher-blog-{tag}-{}-{ts}", std::process::id()));
            fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // Ported from gopher-cta `publish_writes_tree_and_flips_current`.
    #[test]
    fn publish_writes_tree_and_flips_current() {
        let tmp = TmpDir::new("publish");
        let files: Vec<TreeFile> = vec![
            ("index.gph".to_string(), b"root menu\n".to_vec()),
            ("posts/hello.txt".to_string(), b"a post body\n".to_vec()),
        ];

        let snap = publish(&tmp.0, &files, KEEP_SNAPSHOTS).unwrap();

        // current is a symlink to a relative out-* target
        let link = tmp.0.join("current");
        let target = fs::read_link(&link).unwrap();
        assert!(target.to_str().unwrap().starts_with("out-"));
        assert_eq!(tmp.0.join(&target), snap);

        // resolving current/ yields a complete tree, nested dirs and all
        assert_eq!(
            fs::read_to_string(link.join("index.gph")).unwrap(),
            "root menu\n"
        );
        assert_eq!(
            fs::read_to_string(link.join("posts/hello.txt")).unwrap(),
            "a post body\n"
        );
    }

    // Ported from gopher-cta `gc_keeps_recent_plus_current_and_drops_the_rest`.
    #[test]
    fn gc_keeps_recent_plus_current_and_drops_the_rest() {
        let tmp = TmpDir::new("gc");
        // Six chronological snapshots: out-000 .. out-005
        let mut dirs = Vec::new();
        for i in 0..6 {
            let d = tmp.0.join(format!("out-{i:03}"));
            fs::create_dir_all(&d).unwrap();
            dirs.push(d);
        }
        // Pretend out-000 is the current target (oldest) — must be retained even
        // though it's not among the newest KEEP_SNAPSHOTS.
        gc(&tmp.0, &dirs[0], KEEP_SNAPSHOTS).unwrap();

        let remaining: std::collections::BTreeSet<String> = fs::read_dir(&tmp.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with("out-"))
            .collect();
        // newest 3 (003,004,005) + the protected current (000) = 4
        assert_eq!(
            remaining.len(),
            KEEP_SNAPSHOTS + 1,
            "remaining: {remaining:?}"
        );
        assert!(remaining.contains("out-000")); // current protected
        assert!(remaining.contains("out-005")); // newest
        assert!(!remaining.contains("out-001")); // dropped
        assert!(!remaining.contains("out-002")); // dropped
    }

    #[test]
    fn menu_index_renders_geomyidae_gph() {
        let entries = vec![
            render::info("  gopher-blog : a phlog"),
            render::link(render::ItemKind::Menu, "Posts", "/posts"),
            render::link(render::ItemKind::Text, "About", "/about.txt"),
            // a cross-server hub link advertises a concrete host/port
            render::link_remote(
                render::ItemKind::Menu,
                "Live CTA trains",
                "/",
                "gopher.debene.dev",
                70,
            ),
        ];
        let gph = render_menu_index(&entries);

        // info line stays a plain (info) line
        assert!(gph.contains("  gopher-blog : a phlog\n"));
        // local links use the placeholder tokens; never a baked host/port
        assert!(gph.contains("[1|Posts|/posts|server|port]\n"));
        assert!(gph.contains("[0|About|/about.txt|server|port]\n"));
        // remote link emits its concrete host/port columns
        assert!(gph.contains("[1|Live CTA trains|/|gopher.debene.dev|70]\n"));
        assert!(!gph.contains("\t"));
    }
}
