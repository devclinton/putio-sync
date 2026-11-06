//! ContentDirectory: object ids, DIDL-Lite and Browse (feature 022).
//!
//! An object id is `0` for the root, otherwise `percent-encode(key) + "/" +
//! percent-encode(rel)`, where `key` names a shared root and `rel` is a relative path
//! (local roots) or a Put.io file id (`putio`). Ids are checked on every request.

use crate::media::{self, MediaKind};
use crate::putio::{PutioEntry, PutioError};
use crate::soap::escape;
use crate::{RootSource, Shared};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use putio_sync_file_manager::local_files;
use std::path::{Path, PathBuf};

/// Everything but unreserved characters
const ENC: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// A `rel` keeps its separators
const ENC_REL: &AsciiSet = &ENC.remove(b'/');

/// Most entries one Browse returns
pub const MAX_PAGE: usize = 500;

/// A parsed object id
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectId {
    Root,
    Node { key: String, rel: String },
}

impl ObjectId {
    /// Read an id as a player sent it. `None` when it can't be one.
    pub fn parse(s: &str) -> Option<ObjectId> {
        if s == "0" {
            return Some(ObjectId::Root);
        }
        let (key, rel) = s.split_once('/')?;
        let key = percent_decode_str(key).decode_utf8().ok()?.into_owned();
        let rel = percent_decode_str(rel).decode_utf8().ok()?.into_owned();
        if key.is_empty() {
            return None;
        }
        Some(ObjectId::Node { key, rel })
    }

    /// The id as sent to players
    pub fn format(&self) -> String {
        match self {
            ObjectId::Root => "0".to_string(),
            ObjectId::Node { key, rel } => format!(
                "{}/{}",
                utf8_percent_encode(key, ENC),
                utf8_percent_encode(rel, ENC_REL)
            ),
        }
    }

    /// A node id from its parts
    pub fn node(key: &str, rel: &str) -> ObjectId {
        ObjectId::Node {
            key: key.to_string(),
            rel: rel.to_string(),
        }
    }

    /// The path part of a stream URL: the id encoded once more, keeping `/`
    pub fn media_path(&self) -> String {
        format!("/media/{}", utf8_percent_encode(&self.format(), ENC_REL))
    }
}

/// `BrowseFlag`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseFlag {
    Metadata,
    DirectChildren,
}

/// A failed ContentDirectory action
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// 701: an unknown, unshared or refused id (the text is for the log only)
    NoSuchObject(String),
    /// 720: could not process, such as a Put.io failure
    Cannot(String),
    /// 401: not an action this service has
    InvalidAction,
}

impl Fault {
    pub fn code(&self) -> u32 {
        match self {
            Fault::NoSuchObject(_) => 701,
            Fault::Cannot(_) => 720,
            Fault::InvalidAction => 401,
        }
    }

    /// What players are told
    pub fn description(&self) -> &'static str {
        match self {
            Fault::NoSuchObject(_) => "No such object",
            Fault::Cannot(_) => "Cannot process the request",
            Fault::InvalidAction => "Invalid Action",
        }
    }

    /// Why, for the log
    pub fn reason(&self) -> String {
        match self {
            Fault::NoSuchObject(r) | Fault::Cannot(r) => r.clone(),
            Fault::InvalidAction => "invalid action".to_string(),
        }
    }
}

/// A Browse answer
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseOut {
    pub didl: String,
    pub returned: usize,
    pub total: usize,
}

/// A container in DIDL-Lite
pub fn container(id: &str, parent: &str, title: &str, child_count: Option<usize>) -> String {
    let count = child_count
        .map(|c| format!(" childCount=\"{c}\""))
        .unwrap_or_default();
    format!(
        "<container id=\"{}\" parentID=\"{}\" restricted=\"1\"{count}><dc:title>{}</dc:title>\
<upnp:class>object.container.storageFolder</upnp:class></container>",
        escape(id),
        escape(parent),
        escape(title)
    )
}

/// An item with one resource in DIDL-Lite
pub fn item(
    id: &str,
    parent: &str,
    title: &str,
    kind: MediaKind,
    mime: &str,
    size: Option<u64>,
    url: &str,
) -> String {
    let size = size.map(|s| format!(" size=\"{s}\"")).unwrap_or_default();
    format!(
        "<item id=\"{}\" parentID=\"{}\" restricted=\"1\"><dc:title>{}</dc:title>\
<upnp:class>{}</upnp:class><res protocolInfo=\"{}\"{size}>{}</res></item>",
        escape(id),
        escape(parent),
        escape(title),
        kind.upnp_class(),
        escape(&media::protocol_info(mime)),
        escape(url)
    )
}

/// Wrap DIDL-Lite elements
pub fn didl(elements: &[String]) -> String {
    format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">{}</DIDL-Lite>",
        elements.concat()
    )
}

/// A path inside a local root, checked: no escape by `..`, an absolute path or a symlink,
/// and nothing hidden or partial. The result is the real path.
pub fn local_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    for seg in rel.split('/') {
        if !seg.is_empty() && media::is_hidden(seg) {
            return Err(format!("{seg} is hidden"));
        }
    }
    let resolved = local_files::resolve(root, rel).map_err(|e| e.to_string())?;
    // `resolve` doesn't follow the last segment; a link there must not lead out either
    let real = resolved
        .path
        .canonicalize()
        .map_err(|_| format!("{rel} is not there"))?;
    if !real.starts_with(&resolved.root) {
        return Err(format!("{rel} leads outside the folder"));
    }
    Ok(real)
}

/// One listed local entry
struct Entry {
    name: String,
    is_dir: bool,
    size: u64,
    kind: Option<media::MediaType>,
}

fn list_dir(dir: &Path, root: &Path) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if media::is_hidden(&name) {
            continue;
        }
        let Ok(link) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if link.file_type().is_symlink() {
            // Only links that stay inside the root are shown
            match entry.path().canonicalize() {
                Ok(real) if real.starts_with(root) => {}
                _ => continue,
            }
        }
        let Ok(meta) = std::fs::metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            out.push(Entry {
                name,
                is_dir: true,
                size: 0,
                kind: None,
            });
        } else if meta.is_file() {
            if let Some(kind) = media::classify(&name) {
                out.push(Entry {
                    name,
                    is_dir: false,
                    size: meta.len(),
                    kind: Some(kind),
                });
            }
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

fn join(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        format!("{rel}/{name}")
    }
}

fn parent_of(key: &str, rel: &str) -> String {
    match rel.trim_end_matches('/').rsplit_once('/') {
        Some((parent, _)) => ObjectId::node(key, parent).format(),
        None if rel.is_empty() => "0".to_string(),
        None => ObjectId::node(key, "").format(),
    }
}

fn page<T: Clone>(all: &[T], start: usize, count: usize) -> Vec<T> {
    let count = if count == 0 {
        MAX_PAGE
    } else {
        count.min(MAX_PAGE)
    };
    all.iter().skip(start).take(count).cloned().collect()
}

/// Answer a Browse. `base` is `http://<receiving address>:<port>`.
pub async fn browse(
    shared: &Shared,
    base: &str,
    object_id: &str,
    flag: BrowseFlag,
    start: usize,
    count: usize,
) -> Result<BrowseOut, Fault> {
    let id = ObjectId::parse(object_id)
        .ok_or_else(|| Fault::NoSuchObject("not an object id".to_string()))?;
    let (key, rel) = match id {
        ObjectId::Root => return browse_root(shared, flag, start, count),
        ObjectId::Node { key, rel } => (key, rel),
    };
    let root = shared
        .root(&key)
        .ok_or_else(|| Fault::NoSuchObject(format!("{key} is not shared")))?;
    match &root.source {
        RootSource::Local(path) => {
            let path = path.clone();
            let title = root.title.clone();
            browse_local(&key, &title, path, rel, base, flag, start, count).await
        }
        RootSource::Putio(_) => {
            crate::putio::browse(shared, base, &key, &rel, flag, start, count).await
        }
    }
}

fn browse_root(
    shared: &Shared,
    flag: BrowseFlag,
    start: usize,
    count: usize,
) -> Result<BrowseOut, Fault> {
    let children: Vec<String> = shared
        .roots
        .iter()
        .map(|r| container(&ObjectId::node(&r.key, "").format(), "0", &r.title, None))
        .collect();
    match flag {
        BrowseFlag::Metadata => Ok(BrowseOut {
            didl: didl(&[container("0", "-1", &shared.name, Some(children.len()))]),
            returned: 1,
            total: 1,
        }),
        BrowseFlag::DirectChildren => {
            let shown = page(&children, start, count);
            Ok(BrowseOut {
                returned: shown.len(),
                total: children.len(),
                didl: didl(&shown),
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn browse_local(
    key: &str,
    root_title: &str,
    root: PathBuf,
    rel: String,
    base: &str,
    flag: BrowseFlag,
    start: usize,
    count: usize,
) -> Result<BrowseOut, Fault> {
    let rel_owned = rel.clone();
    let listed = tokio::task::spawn_blocking(move || -> Result<Listed, String> {
        // A shared folder that isn't there is an empty container
        if rel_owned.is_empty() && !root.is_dir() {
            return Ok(Listed::Dir(Vec::new()));
        }
        let real = local_path(&root, &rel_owned)?;
        let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&real).map_err(|_| "not there".to_string())?;
        if meta.is_dir() {
            Ok(Listed::Dir(
                list_dir(&real, &canonical_root).map_err(|e| e.to_string())?,
            ))
        } else {
            let name = real
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let kind = media::classify(&name).ok_or_else(|| "not media".to_string())?;
            Ok(Listed::File(name, meta.len(), kind))
        }
    })
    .await
    .map_err(|e| Fault::Cannot(e.to_string()))?
    .map_err(Fault::NoSuchObject)?;

    let this_id = ObjectId::node(key, &rel).format();
    let parent = parent_of(key, &rel);
    match (listed, flag) {
        (Listed::Dir(_), BrowseFlag::Metadata) => {
            let title = if rel.is_empty() {
                root_title.to_string()
            } else {
                rel.rsplit('/').next().unwrap_or(&rel).to_string()
            };
            Ok(BrowseOut {
                didl: didl(&[container(&this_id, &parent, &title, None)]),
                returned: 1,
                total: 1,
            })
        }
        (Listed::Dir(entries), BrowseFlag::DirectChildren) => {
            let elements: Vec<String> = entries
                .iter()
                .map(|e| {
                    let child = ObjectId::node(key, &join(&rel, &e.name));
                    if e.is_dir {
                        container(&child.format(), &this_id, &e.name, None)
                    } else {
                        let kind = e.kind.as_ref().expect("files are media");
                        item(
                            &child.format(),
                            &this_id,
                            &e.name,
                            kind.kind,
                            &kind.mime,
                            Some(e.size),
                            &format!("{base}{}", child.media_path()),
                        )
                    }
                })
                .collect();
            let shown = page(&elements, start, count);
            Ok(BrowseOut {
                returned: shown.len(),
                total: elements.len(),
                didl: didl(&shown),
            })
        }
        (Listed::File(name, size, kind), BrowseFlag::Metadata) => {
            let id = ObjectId::node(key, &rel);
            Ok(BrowseOut {
                didl: didl(&[item(
                    &id.format(),
                    &parent,
                    &name,
                    kind.kind,
                    &kind.mime,
                    Some(size),
                    &format!("{base}{}", id.media_path()),
                )]),
                returned: 1,
                total: 1,
            })
        }
        (Listed::File(..), BrowseFlag::DirectChildren) => Ok(BrowseOut {
            didl: didl(&[]),
            returned: 0,
            total: 0,
        }),
    }
}

enum Listed {
    Dir(Vec<Entry>),
    File(String, u64, media::MediaType),
}

/// Fault for a Put.io failure
pub(crate) fn putio_fault(e: PutioError) -> Fault {
    match e {
        PutioError::Outside(r) => Fault::NoSuchObject(r),
        PutioError::Upstream(r) => Fault::Cannot(r),
    }
}

/// A Put.io entry as DIDL
pub(crate) fn putio_didl(
    key: &str,
    parent: &str,
    entry: &PutioEntry,
    media: Option<(media::MediaType, Option<u64>)>,
    base: &str,
) -> Option<String> {
    let id = ObjectId::node(key, &entry.id.to_string());
    if entry.is_folder {
        return Some(container(&id.format(), parent, &entry.name, None));
    }
    let (kind, size) = media?;
    Some(item(
        &id.format(),
        parent,
        &entry.name,
        kind.kind,
        &kind.mime,
        size,
        &format!("{base}{}", id.media_path()),
    ))
}

/// The page of a list, shared with the Put.io root
pub(crate) fn paged<T: Clone>(all: &[T], start: usize, count: usize) -> Vec<T> {
    page(all, start, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soap::parse_action;

    #[test]
    fn root_and_nodes_round_trip() {
        assert_eq!(ObjectId::parse("0"), Some(ObjectId::Root));
        assert_eq!(ObjectId::Root.format(), "0");
        for (key, rel) in [
            ("downloads", ""),
            ("downloads", "Movies/The Film (2020).mkv"),
            ("cat:radarr", "a & b/c#d%e.mp4"),
            ("watch:/watches/sonarr", "x/\u{1F600} emoji.mkv"),
            ("putio", "123"),
        ] {
            let id = ObjectId::node(key, rel);
            let s = id.format();
            assert!(
                !s.contains(' ') && !s.contains('&') && !s.contains('#'),
                "{s}"
            );
            assert_eq!(ObjectId::parse(&s), Some(id), "{s}");
        }
    }

    #[test]
    fn bad_ids_do_not_parse() {
        assert_eq!(ObjectId::parse(""), None);
        assert_eq!(ObjectId::parse("downloads"), None);
        assert_eq!(ObjectId::parse("/x"), None);
        assert_eq!(ObjectId::parse("downloads/%FF"), None);
    }

    #[test]
    fn media_path_is_encoded_twice_and_keeps_slashes() {
        let id = ObjectId::node("downloads", "A B/c.mp4");
        assert_eq!(id.format(), "downloads/A%20B/c.mp4");
        assert_eq!(id.media_path(), "/media/downloads/A%2520B/c.mp4");
    }

    #[test]
    fn didl_escapes_and_parses_back() {
        let elements = vec![
            container("downloads/A%20%26%20B", "0", "A & <B> \"q\"", Some(2)),
            item(
                "downloads/x.mp4",
                "downloads/",
                "x & y.mp4",
                MediaKind::Video,
                "video/mp4",
                Some(10),
                "http://1.2.3.4:8200/media/downloads/x.mp4?a=1&b=2",
            ),
        ];
        let xml = didl(&elements);
        let mut reader = quick_xml::Reader::from_str(&xml);
        let mut titles = Vec::new();
        let mut in_title = false;
        loop {
            match reader.read_event().unwrap() {
                quick_xml::events::Event::Start(e) if e.name().as_ref() == b"dc:title" => {
                    in_title = true
                }
                quick_xml::events::Event::Text(t) if in_title => {
                    titles.push(t.unescape().unwrap().to_string());
                    in_title = false;
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(titles, vec!["A & <B> \"q\"", "x & y.mp4"]);
        assert!(xml.contains("childCount=\"2\""));
        assert!(xml.contains("size=\"10\""));
        // The whole thing survives being escaped into a SOAP reply
        let reply = crate::soap::envelope("Browse", crate::soap::CDS, &[("Result", xml.clone())]);
        let back = parse_action(&reply).unwrap();
        assert_eq!(back.arg("Result"), Some(xml.as_str()));
    }

    #[test]
    fn parents() {
        assert_eq!(parent_of("downloads", ""), "0");
        assert_eq!(parent_of("downloads", "a"), "downloads/");
        assert_eq!(parent_of("downloads", "a/b"), "downloads/a");
    }

    #[test]
    fn paging_caps_and_zero_means_all() {
        let v: Vec<usize> = (0..1000).collect();
        assert_eq!(page(&v, 0, 0).len(), 500);
        assert_eq!(page(&v, 10, 5), vec![10, 11, 12, 13, 14]);
        assert_eq!(page(&v, 990, 0).len(), 10);
        assert_eq!(page(&v, 0, 5000).len(), 500);
    }
}
