# gopher-blog

A **phlog generator**: render the [debene.dev](https://debene.dev) Hugo blog into
a static [gopher](https://en.wikipedia.org/wiki/Gopher_(protocol)) tree, then let
[geomyidae](https://r-36.net/scm/geomyidae/) serve it. Not a server — it renders
files and exits. Built as a smaller sibling of
[`gopher-cta`](https://github.com/felipedbene/gopher-cta): the same publisher
spine, with the live-train brain swapped for a markdown brain.

It runs **once**: discover → render → publish → exit. No loop, no network, no
async.

## Usage

```sh
gopher-blog --content <dir> --out <dir>
            [--host gopher.debene.dev] [--port 70]
            [--cta-link gopher://gopher.debene.dev:70]
            [--keep 3]
```

- `--content` — Hugo content root (contains `posts/*/index.md`). Required.
- `--out` — output root. Each run writes `out-<ts>/` and atomically flips the
  `current` symlink to it; old snapshots are GC'd to `--keep`.
- `--host` / `--port` — the host/port stamped into generated `.gph` link columns.
- `--cta-link` — a `gopher://host[:port]` back-link to the cta hole, added to the
  root menu (the hub topology). Pass `none` to omit.
- `--keep` — snapshots to retain on GC (default 3).

Example (local preview):

```sh
gopher-blog --content ../debene-dev/content --out ./out --host 127.0.0.1 --port 7071
geomyidae -b ./out/current -p 7071 -h 127.0.0.1
lynx gopher://127.0.0.1:7071/
```

## Published tree

```
/index.gph            root: banner, blurb → Posts, Tags, Series, About, + cta link
/posts/index.gph      newest-first; "YYYY-MM-DD  Title" → /posts/<slug>.txt
/posts/<slug>.txt     the post
/tags/index.gph       each tag (with count) → /tags/<tag>.gph
/tags/<tag>.gph       posts with that tag, newest-first
/series/index.gph     each series → /series/<series>.gph
/series/<series>.gph  posts in series order (oldest-first)
/about.txt            rendered from content/about.md (or a stub)
```

## Markdown → gopher

Posts are reflowed to **70 columns** as plain text. Inline links become `text[N]`
with a footnoted **Links** section; images become `[img: alt]` (the image stays
on the web, referenced in the footer). Code fences are emitted **verbatim** (never
reflowed). Tables render aligned if they fit in 70 cols, else fall back to a
"see web" pointer. See [`src/markdown.rs`](src/markdown.rs) for the full contract.

## Architecture

| Module | Responsibility |
|---|---|
| `content.rs` | Walk `posts/*/index.md`; parse YAML frontmatter; split the body; skip drafts; sort newest-first. |
| `markdown.rs` | Pure md→gopher-text + footnote table. |
| `render.rs` | `Entry` menu model (copied spine) + page builders (root/posts/tags/series menus, post page, about). |
| `publish.rs` | `.gph` serializer (copied spine) + atomic publish (`out-<ts>/` → flip `current` → GC). |
| `main.rs` | CLI + `build_tree` (assemble the file map). |

The `Entry` model and publisher are **copied** from gopher-cta (marked
`EXTRACTION CANDIDATE`) rather than shared, as tracked debt — a `gopher-core`
crate will be extracted once the `Entry` API settles. See
`gopher-blog-DESIGN.md` for the full design and the hub-topology rationale.

## Develop

```sh
cargo build
cargo test
cargo clippy --all-targets
cargo fmt --check
# optional: validate the real content obeys the 70-col invariant
DEBENE_CONTENT=../debene-dev/content cargo test real_posts -- --ignored --nocapture
```

## Deploy

Production runs as a single immutable container (geomyidae + the baked tree) on
the same VPS as gopher-cta, on `:7071`. CI publishes
`ghcr.io/felipedbene/gopher-blog:latest` (multi-arch); the cta Watchtower swaps
it on each new digest. See [`deploy/DEPLOY.md`](deploy/DEPLOY.md).

## License

MIT.
