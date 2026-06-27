//! Pure rendering: parsed content in -> text / menu structures out.
//!
//! No sockets, no gopher protocol bytes, no daemon-specific formatting — this is
//! the testable core. Text pages come out as plain `String`s; menus come out as
//! a daemon-agnostic [`Vec<Entry>`]. Turning entries into a specific daemon's
//! index format (geomyidae `.gph`) happens in [`crate::publish`], not here.
//!
//! Selectors are the gopher selectors as served from the tree root, i.e. the
//! on-disk paths the publisher writes (`/index.gph`, `/posts/<slug>.txt`, ...).

// EXTRACTION CANDIDATE -> gopher-core (extract once blog v1 renders; see DESIGN).
// Keep byte-identical to gopher-cta until then. Any edit here MUST also land in cta.
// NOTE: this block carries the *settled* `Entry` shape — `Entry::Link` already has
// the `host`/`port` fields the gopher-cta cross-link commit adds; both copies are
// the pre-`gopher-core` API and must stay in sync.

/// Gopher item type for a link. Daemon-agnostic; serialized per-daemon elsewhere.
///
/// `Url`/`Bin` are carried verbatim from the copied spine; the blog's menus only
/// use `Text`/`Menu`, but the variants stay so the serializer matches cta's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ItemKind {
    Text, // gopher type 0
    Menu, // gopher type 1
    Url,  // gopher type h -- external link, selector is `URL:<addr>`
    Bin,  // gopher type 9 -- binary download
}

/// One line of a menu: either an info line (not selectable) or a link.
///
/// `host`/`port` are normally `None` — the serializer then emits geomyidae's own
/// host/port placeholders, so the tree stays address-agnostic. They are `Some`
/// only for cross-server links (the hub link back to the cta hole), which must
/// advertise a concrete address the client dials directly.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Info(String),
    Link {
        kind: ItemKind,
        display: String,
        selector: String,
        host: Option<String>,
        port: Option<u16>,
    },
}

/// An info (non-selectable) line.
pub fn info(s: impl Into<String>) -> Entry {
    Entry::Info(s.into())
}

/// A link served from this tree (host/port default to the daemon's own).
pub fn link(kind: ItemKind, display: impl Into<String>, selector: impl Into<String>) -> Entry {
    Entry::Link {
        kind,
        display: display.into(),
        selector: selector.into(),
        host: None,
        port: None,
    }
}

/// A link to a *different* gopher server: the `.gph` line advertises this
/// host/port so the client opens a fresh connection there.
pub fn link_remote(
    kind: ItemKind,
    display: impl Into<String>,
    selector: impl Into<String>,
    host: impl Into<String>,
    port: u16,
) -> Entry {
    Entry::Link {
        kind,
        display: display.into(),
        selector: selector.into(),
        host: Some(host.into()),
        port: Some(port),
    }
}
// END EXTRACTION CANDIDATE.

/// Stamp local links (those with no explicit host) with the tree's own
/// host/port, so generated `.gph` lines advertise a concrete address rather than
/// relying on the daemon's placeholder substitution (`--host`/`--port`). Info
/// lines and cross-server links (which already carry a host) are left untouched.
///
/// Blog-specific: gopher-cta leaves local links as placeholder tokens. Lives
/// outside the extraction block.
pub fn with_host(entries: Vec<Entry>, host: &str, port: u16) -> Vec<Entry> {
    entries
        .into_iter()
        .map(|e| match e {
            Entry::Link {
                kind,
                display,
                selector,
                host: None,
                port: None,
            } => Entry::Link {
                kind,
                display,
                selector,
                host: Some(host.to_string()),
                port: Some(port),
            },
            other => other,
        })
        .collect()
}
