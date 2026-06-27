# NIGHT-LOG — gopher-blog

Overnight unsupervised build. Scan this in two minutes; details in README.

## TL;DR

Done and working. A one-shot phlog generator in Rust: it renders the debene.dev
Hugo blog into a static gopher tree and atomically publishes it for geomyidae to
serve. No loop, no network, no async. Built as a *smaller gopher-cta* — the
publisher spine copied over, the train brain swapped for a markdown brain. All
tests + clippy + fmt green. Verified end-to-end against the real `debene-dev`
content (135 files); the live geomyidae browse is the one remaining manual step
(see Verification).

## What got built (5 commits, one per sub-task)

1. **scaffold + spine** — `render.rs` `Entry`/`ItemKind` + `info`/`link`/
   `link_remote`; `publish.rs` `render_menu_index` (geomyidae `.gph` serializer)
   + atomic `publish` (`out-<ts>/` → flip `current` → GC). Copied from gopher-cta
   under `EXTRACTION CANDIDATE` markers. **`Entry::Link` carries the settled
   post-cross-link shape** (`host`/`port` already present) so it matches the
   companion cta change; a `None` host/port serializes byte-identically to today's
   cta output. The two cta publisher tests ported (`publish_writes_tree_…`,
   `gc_keeps_recent_…`) plus a serializer test.
2. **content.rs** — `parse_post` (pure): split the `---` fences (body preserved
   byte-for-byte), parse YAML via `yaml-rust2`, pull
   title/date/draft/description/tags/categories/series. Slug = dir name unless an
   explicit `slug:`. Dependency-free date handling — bare `YYYY-MM-DD` and full
   RFC3339-with-offset both → a UTC epoch sort key. `discover` walks
   `posts/*/index.md`, skips drafts, sorts newest-first.
3. **markdown.rs** — the md→gopher contract over a `pulldown-cmark` event stream:
   headings (underlined), 70-col word wrap, code fences verbatim (bracketed by a
   thin rule), inline links → `text[N]` + footnote, images → `[img: alt]` +
   footnote (shared per-page sequence), lists, blockquotes, tables (aligned if
   ≤70 cols else `[table — see web]`). `* * *` thematic break, distinct from the
   code rule.
4. **render.rs page builders + wiring** — `root_menu`, `posts_index`,
   `tags_index`/`series_index`, `tag_menu` (newest-first) / `series_menu`
   (oldest-first), `post_page` (framed header + body + Links footer + nav),
   `about_page`. `main::build_tree` assembles the file map; menus stamped with the
   tree's host/port via `with_host`.
5. **end-to-end + this log.**

## Deps (pinned exact, minimal)

- `pulldown-cmark =0.13.4` — markdown (actively maintained CommonMark parser).
- `yaml-rust2 =0.11.0` — frontmatter. `serde_yaml` is archived/unmaintained;
  `yaml-rust2` is the maintained pure-Rust successor and needs no `serde`/`chrono`
  (date + arg parsing are hand-rolled).

## Tracked debt (deliberate, not silent)

The spine is **copied** from gopher-cta, not shared, under `EXTRACTION CANDIDATE`
comments. `gopher-core` is *not* extracted yet because `Entry::Link` is still
moving (the `host`/`port` cross-link addition). Rule while copied: any edit to the
marked blocks must also land in cta. One deliberate divergence: `gc()` takes a
`keep` parameter (for `--keep`) where cta hard-codes the constant. See DESIGN
fio #1 for the planned extraction.

## Verification

- `cargo build` / `cargo test` (34 + 1 ignored) / `cargo clippy --all-targets` /
  `cargo fmt --check` — all clean.
- Rendered the real `debene-dev` content: **22 non-draft posts** (24 dirs − 2
  drafts), **135 files**. Every post-body line ≤70 cols outside code/footnote
  URLs (checked across all 22). Series order confirmed oldest-first.
- **Static link integrity:** all 546 menu link lines well-formed (5 `.gph`
  fields, valid type char, no tabs); every local selector resolves to a real file
  in the tree; the single cta hub link points at `:70`.
- **Live geomyidae browse — TODO (manual):** geomyidae isn't installed locally
  (not in brew; building the upstream C source was out of scope for an
  unsupervised run). Point it at a snapshot and browse with Bombadillo / Lagrange:
  ```sh
  gopher-blog --content ../debene-dev/content --out ./out --port 7071
  geomyidae -b ./out/current -p 7071 -h 127.0.0.1
  lynx gopher://127.0.0.1:7071/
  ```

## Follow-up fios (not this session)

1. Extract `gopher-core` once `Entry` settles; cta + blog both depend on it.
2. Deploy: Dockerfile `FROM geomyidae`, CI image, Watchtower swap.
3. Web gateway at `https://debene.dev/gopher/` (additive).
