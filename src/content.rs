//! Input model: walk the Hugo content tree, parse YAML frontmatter, split the
//! markdown body. Pure parsing of one post lives in [`parse_post`]; filesystem
//! discovery (draft skip, newest-first sort) lives in [`discover`].

use std::fs;
use std::io;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

/// One parsed post. `body` is the raw markdown after the closing frontmatter
/// fence (handed to [`crate::markdown`] for rendering).
// The render layer (commit 4) consumes every field; until then a few are only
// read in tests, so silence dead-code on the plain bin build.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    /// URL/selector stem: the post directory name, or an explicit `slug:`.
    pub slug: String,
    pub title: String,
    /// `YYYY-MM-DD` for display in menus and the post header.
    pub date: String,
    /// Sort key: UTC epoch seconds parsed from the frontmatter date (0 if
    /// unparseable). Newest-first ordering sorts on this descending.
    pub sort_key: i64,
    pub draft: bool,
    pub description: String,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub series: Vec<String>,
    pub body: String,
}

/// Discover every non-draft post under `<content_root>/posts/*/index.md`, parsed
/// and sorted newest-first (ties broken by slug for determinism). A post that
/// fails to parse is skipped with a warning rather than aborting the run.
pub fn discover(content_root: &Path) -> io::Result<Vec<Post>> {
    let posts_dir = content_root.join("posts");
    let mut posts = Vec::new();
    for entry in fs::read_dir(&posts_dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let index = path.join("index.md");
        if !index.is_file() {
            continue;
        }
        let slug = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let raw = fs::read_to_string(&index)?;
        match parse_post(&slug, &raw) {
            Ok(post) if post.draft => {} // skip drafts
            Ok(post) => posts.push(post),
            Err(e) => eprintln!("gopher-blog: warning: skipping {slug}: {e}"),
        }
    }
    posts.sort_by(|a, b| {
        b.sort_key
            .cmp(&a.sort_key)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    Ok(posts)
}

/// Parse one post from its raw `index.md` text. `dir_slug` is the directory name,
/// used as the slug unless the frontmatter carries an explicit `slug:`. Errors
/// only when the frontmatter fence is absent or the YAML is unparseable; missing
/// individual fields fall back to sensible defaults.
pub fn parse_post(dir_slug: &str, raw: &str) -> Result<Post, String> {
    let (fm, body) =
        split_frontmatter(raw).ok_or_else(|| "missing '---' frontmatter fence".to_string())?;

    let docs = YamlLoader::load_from_str(fm).map_err(|e| format!("invalid YAML: {e}"))?;
    let doc = docs.first().cloned().unwrap_or(Yaml::Null);

    let slug = ystr(&doc, "slug").unwrap_or_else(|| dir_slug.to_string());
    let title = ystr(&doc, "title").unwrap_or_else(|| dir_slug.to_string());
    let raw_date = ystr(&doc, "date").unwrap_or_default();
    let (date, sort_key) = parse_date(&raw_date);

    Ok(Post {
        slug,
        title,
        date,
        sort_key,
        draft: ybool(&doc, "draft"),
        description: ystr(&doc, "description").unwrap_or_default(),
        tags: yarr(&doc, "tags"),
        categories: yarr(&doc, "categories"),
        series: yarr(&doc, "series"),
        body: body.to_string(),
    })
}

/// Split raw markdown into `(frontmatter_yaml, body)` at the `---` fences. The
/// body slice is preserved byte-for-byte (code fences must round-trip exactly),
/// starting immediately after the closing fence line. Returns `None` if the text
/// does not open with a `---` fence and carry a matching closing one.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw); // tolerate a BOM
    let after_open = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            let fm = &after_open[..offset];
            let body = &after_open[offset + line.len()..];
            return Some((fm, body));
        }
        offset += line.len();
    }
    None
}

/// A scalar frontmatter field as a string. Coerces ints/floats so e.g. a
/// numeric-looking value still reads.
fn ystr(doc: &Yaml, key: &str) -> Option<String> {
    match &doc[key] {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        _ => None,
    }
}

/// A boolean frontmatter field (`false` if absent or non-boolean).
fn ybool(doc: &Yaml, key: &str) -> bool {
    doc[key].as_bool().unwrap_or(false)
}

/// A string-array frontmatter field (empty if absent). Non-string elements are
/// dropped.
fn yarr(doc: &Yaml, key: &str) -> Vec<String> {
    match &doc[key] {
        Yaml::Array(a) => a
            .iter()
            .filter_map(|y| y.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

/// Parse a frontmatter date into `(YYYY-MM-DD display, UTC epoch seconds)`.
/// Accepts both bare `YYYY-MM-DD` and full RFC3339 (`...THH:MM:SS±HH:MM` / `Z`).
/// Unparseable input yields `(raw, 0)` so the post still renders, just unsorted.
fn parse_date(raw: &str) -> (String, i64) {
    let display = raw
        .get(..10)
        .filter(|s| s.as_bytes().get(4) == Some(&b'-'))
        .unwrap_or(raw)
        .to_string();
    (display, date_to_epoch(raw).unwrap_or(0))
}

/// RFC3339-ish date/datetime -> UTC epoch seconds.
fn date_to_epoch(raw: &str) -> Option<i64> {
    let year: i64 = raw.get(0..4)?.parse().ok()?;
    let month: i64 = raw.get(5..7)?.parse().ok()?;
    let day: i64 = raw.get(8..10)?.parse().ok()?;
    let mut secs = days_from_civil(year, month, day) * 86_400;

    // Optional time component: "T" or " " then HH:MM:SS.
    let sep = raw.as_bytes().get(10).copied();
    if (sep == Some(b'T') || sep == Some(b' ')) && raw.len() >= 19 {
        let hh: i64 = raw.get(11..13)?.parse().ok()?;
        let mm: i64 = raw.get(14..16)?.parse().ok()?;
        let ss: i64 = raw.get(17..19)?.parse().ok()?;
        secs += hh * 3600 + mm * 60 + ss;
        // Optional zone offset; subtract it to land on UTC.
        secs -= parse_offset(&raw[19..]).unwrap_or(0);
    }
    Some(secs)
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // Mar=0..Feb=11
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Parse a zone offset (`Z`, `±HH:MM`) into seconds east of UTC.
fn parse_offset(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s == "Z" {
        return Some(0);
    }
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (h, m) = s[1..].split_once(':')?;
    let hh: i64 = h.parse().ok()?;
    let mm: i64 = m.parse().ok()?;
    Some(sign * (hh * 3600 + mm * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "\
---
title: \"Live Trains on a Quiet Internet\"
date: 2026-06-23T11:00:00-05:00
draft: false
description: \"trains over gopher\"
tags: [\"gopher\", \"rust\"]
categories: [\"Networking\"]
series: [\"Quiet Internet\"]
---
# Hello

Body text here.
";

    #[test]
    fn parses_valid_post() {
        let p = parse_post("gopher-cta-live-trains", VALID).unwrap();
        assert_eq!(p.slug, "gopher-cta-live-trains");
        assert_eq!(p.title, "Live Trains on a Quiet Internet");
        assert_eq!(p.date, "2026-06-23");
        assert!(!p.draft);
        assert_eq!(p.description, "trains over gopher");
        assert_eq!(p.tags, vec!["gopher", "rust"]);
        assert_eq!(p.categories, vec!["Networking"]);
        assert_eq!(p.series, vec!["Quiet Internet"]);
        // body preserved verbatim, starting after the closing fence
        assert_eq!(p.body, "# Hello\n\nBody text here.\n");
    }

    #[test]
    fn detects_draft() {
        let raw = "---\ntitle: Chimarrão\ndate: 2026-02-28\ndraft: true\n---\nbody\n";
        let p = parse_post("chimarrao", raw).unwrap();
        assert!(p.draft);
        assert_eq!(p.date, "2026-02-28");
    }

    #[test]
    fn missing_fields_default() {
        // No title, no tags/categories/series/description; bare date.
        let raw = "---\ndate: 2026-05-14\n---\njust a body\n";
        let p = parse_post("apple-silicon-vs-power8", raw).unwrap();
        assert_eq!(p.title, "apple-silicon-vs-power8"); // falls back to slug
        assert_eq!(p.date, "2026-05-14");
        assert!(!p.draft); // absent -> false
        assert!(p.tags.is_empty());
        assert!(p.categories.is_empty());
        assert!(p.series.is_empty());
        assert_eq!(p.description, "");
        assert_eq!(p.body, "just a body\n");
    }

    #[test]
    fn explicit_slug_overrides_dir() {
        let raw = "---\ntitle: T\ndate: 2026-01-01\nslug: custom-slug\n---\nx\n";
        let p = parse_post("dir-name", raw).unwrap();
        assert_eq!(p.slug, "custom-slug");
    }

    #[test]
    fn no_frontmatter_errors() {
        assert!(parse_post("x", "# just markdown\n").is_err());
    }

    #[test]
    fn bare_date_and_offset_both_parse_chronologically() {
        // Same instant expressed bare vs with offset should order sanely; an
        // earlier date must sort before a later one.
        let early = parse_post("a", "---\ndate: 2025-01-15T00:00:00-06:00\n---\n").unwrap();
        let late = parse_post("b", "---\ndate: 2026-06-23\n---\n").unwrap();
        assert!(late.sort_key > early.sort_key);
    }

    #[test]
    fn offset_shifts_utc() {
        // 2026-06-23T11:00:00-05:00 == 2026-06-23T16:00:00Z
        let off = date_to_epoch("2026-06-23T11:00:00-05:00").unwrap();
        let utc = date_to_epoch("2026-06-23T16:00:00Z").unwrap();
        assert_eq!(off, utc);
    }
}
