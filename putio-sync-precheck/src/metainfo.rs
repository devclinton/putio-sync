//! Reading a .torrent file: its info hash, file list, private flag and tracker hosts
//! (research R2). Moved from `putio-sync-arr/src/infohash.rs`, which keeps magnet links.

use std::collections::BTreeSet;

use putio_sync_client::models::tracker_host;
use sha1::{Digest, Sha1};

/// More files than this is not a release anyone grabs; refusing bounds the work
pub const MAX_FILES: usize = 100_000;

/// Real torrents nest a few levels; this bounds recursion on hostile input
const MAX_DEPTH: usize = 64;

/// One file of a release, its path joined with `/` under the torrent name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFile {
    pub path: String,
    pub size: u64,
}

impl ReleaseFile {
    /// The last extension, lower case, as 009's `rules::extension` reads it
    pub fn extension(&self) -> Option<String> {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        putio_sync_core::rules::extension(name)
    }
}

/// What a .torrent file says about its release
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metainfo {
    /// Info hash, 40 lower-case hex characters
    pub hash: String,
    pub name: String,
    /// Padding files left out
    pub files: Vec<ReleaseFile>,
    pub private: bool,
    /// Host names only, sorted: never the announce address, which can carry a passkey
    pub tracker_hosts: Vec<String>,
}

impl Metainfo {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// The info hash of a .torrent file (SHA-1 of the bencoded `info` dictionary, byte for
/// byte as in the file) and its `info.name`. `None` when it isn't a valid torrent.
/// Needs no file list, so 011 can identify any torrent by it.
pub fn info_hash(bytes: &[u8]) -> Option<(String, Option<String>)> {
    if bytes.first() != Some(&b'd') {
        return None;
    }
    let mut pos = 1;
    while bytes.get(pos)? != &b'e' {
        let (key, after_key) = read_string(bytes, pos)?;
        let (value, end) = parse_value(bytes, after_key, 1)?;
        if key == b"info" {
            let Value::Dict(info) = value else {
                return None;
            };
            let hash = hex(&Sha1::digest(&bytes[after_key..end]));
            return Some((hash, info_name(&info)));
        }
        pos = end;
    }
    None
}

/// The full file list of a .torrent file. `None` when it isn't a valid torrent, has no
/// file list, has a negative length or has more than [`MAX_FILES`] files.
pub fn parse(bytes: &[u8]) -> Option<Metainfo> {
    if bytes.first() != Some(&b'd') {
        return None;
    }
    let mut pos = 1;
    let mut info = None;
    let mut trackers = BTreeSet::new();
    loop {
        match bytes.get(pos) {
            Some(b'e') => break,
            Some(_) => {}
            // Truncated before the info dictionary was complete
            None if info.is_none() => return None,
            None => break,
        }
        let entry = read_string(bytes, pos)
            .and_then(|(key, after)| parse_value(bytes, after, 1).map(|v| (key, after, v)));
        let Some((key, after_key, (value, end))) = entry else {
            // Junk after a complete info dictionary doesn't change the release
            if info.is_some() {
                break;
            }
            return None;
        };
        match key {
            b"info" => match value {
                Value::Dict(dict) => {
                    info = Some((hex(&Sha1::digest(&bytes[after_key..end])), dict))
                }
                _ => return None,
            },
            b"announce" => trackers.extend(value.as_str().as_deref().and_then(tracker_host)),
            b"announce-list" => {
                for tier in value.as_list() {
                    for url in tier.as_list() {
                        trackers.extend(url.as_str().as_deref().and_then(tracker_host));
                    }
                }
            }
            _ => {}
        }
        pos = end;
    }
    let (hash, info) = info?;
    let name = info_name(&info)?;
    let files = files(&info, &name)?;
    let private = matches!(get(&info, b"private"), Some(Value::Int(1)));
    Some(Metainfo {
        hash,
        name,
        files,
        private,
        tracker_hosts: trackers.into_iter().collect(),
    })
}

/// v1 `files` (also for hybrids), single-file `length`, else the v2 `file tree`
fn files(info: &Dict, name: &str) -> Option<Vec<ReleaseFile>> {
    if let Some(Value::List(entries)) = get(info, b"files") {
        if entries.len() > MAX_FILES {
            return None;
        }
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let Value::Dict(entry) = entry else {
                return None;
            };
            let size = length(entry)?;
            let path = get(entry, b"path.utf-8").or_else(|| get(entry, b"path"))?;
            let segments: Vec<String> = path
                .as_list()
                .iter()
                .map(|s| s.as_str())
                .collect::<Option<_>>()?;
            if segments.is_empty() || is_padding(entry, &segments) {
                continue;
            }
            out.push(ReleaseFile {
                path: format!("{name}/{}", segments.join("/")),
                size,
            });
        }
        return Some(out);
    }
    if get(info, b"length").is_some() {
        return Some(vec![ReleaseFile {
            path: name.to_string(),
            size: length(info)?,
        }]);
    }
    if let Some(Value::Dict(tree)) = get(info, b"file tree") {
        let mut out = Vec::new();
        walk_tree(tree, name, &mut out)?;
        return Some(out);
    }
    None
}

/// v2: a directory is a dict of names; a file is a dict whose only key is "" holding
/// its `length`
fn walk_tree(tree: &Dict, prefix: &str, out: &mut Vec<ReleaseFile>) -> Option<()> {
    for (key, value) in tree {
        let Value::Dict(child) = value else {
            return None;
        };
        if key.is_empty() {
            continue;
        }
        let path = format!("{prefix}/{}", String::from_utf8_lossy(key));
        if let Some(Value::Dict(leaf)) = get(child, b"") {
            let segment = path.rsplit('/').next().unwrap_or("");
            if !segment.starts_with(".pad") {
                out.push(ReleaseFile {
                    path,
                    size: length(leaf)?,
                });
            }
        } else {
            walk_tree(child, &path, out)?;
        }
        if out.len() > MAX_FILES {
            return None;
        }
    }
    Some(())
}

/// BEP 47 padding: `attr` contains `p`, a `.pad` directory, or the older
/// `_____padding_file` name
fn is_padding(entry: &Dict, segments: &[String]) -> bool {
    let attr = get(entry, b"attr").and_then(Value::as_str);
    attr.is_some_and(|a| a.contains('p'))
        || segments.first().is_some_and(|s| s.starts_with(".pad"))
        || segments
            .last()
            .is_some_and(|s| s.starts_with("_____padding_file"))
}

fn length(dict: &Dict) -> Option<u64> {
    match get(dict, b"length")? {
        Value::Int(n) => u64::try_from(*n).ok(),
        _ => None,
    }
}

/// `name.utf-8`, else `name`, of an info dictionary
fn info_name(info: &Dict) -> Option<String> {
    get(info, b"name.utf-8")
        .or_else(|| get(info, b"name"))
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

type Dict<'a> = Vec<(&'a [u8], Value<'a>)>;

enum Value<'a> {
    Int(i64),
    Bytes(&'a [u8]),
    List(Vec<Value<'a>>),
    Dict(Dict<'a>),
}

impl Value<'_> {
    fn as_str(&self) -> Option<String> {
        match self {
            Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }

    fn as_list(&self) -> &[Value<'_>] {
        match self {
            Value::List(items) => items,
            _ => &[],
        }
    }
}

fn get<'d, 'a>(dict: &'d Dict<'a>, key: &[u8]) -> Option<&'d Value<'a>> {
    dict.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// The bencoded value at `pos` and the position after it
fn parse_value(bytes: &[u8], pos: usize, depth: usize) -> Option<(Value<'_>, usize)> {
    if depth > MAX_DEPTH {
        return None;
    }
    match *bytes.get(pos)? {
        b'i' => {
            let end = pos + bytes.get(pos..)?.iter().position(|&b| b == b'e')?;
            let digits = std::str::from_utf8(&bytes[pos + 1..end]).ok()?;
            Some((Value::Int(digits.parse().ok()?), end + 1))
        }
        b'l' => {
            let mut items = Vec::new();
            let mut p = pos + 1;
            while *bytes.get(p)? != b'e' {
                let (item, next) = parse_value(bytes, p, depth + 1)?;
                items.push(item);
                p = next;
            }
            Some((Value::List(items), p + 1))
        }
        b'd' => {
            let mut entries = Vec::new();
            let mut p = pos + 1;
            while *bytes.get(p)? != b'e' {
                let (key, after_key) = read_string(bytes, p)?;
                let (value, next) = parse_value(bytes, after_key, depth + 1)?;
                entries.push((key, value));
                p = next;
            }
            Some((Value::Dict(entries), p + 1))
        }
        b'0'..=b'9' => read_string(bytes, pos).map(|(s, end)| (Value::Bytes(s), end)),
        _ => None,
    }
}

/// A bencoded byte string at `pos`: its bytes and the position after it
fn read_string(bytes: &[u8], pos: usize) -> Option<(&[u8], usize)> {
    let colon = pos + bytes.get(pos..)?.iter().position(|&b| b == b':')?;
    let digits = std::str::from_utf8(&bytes[pos..colon]).ok()?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let len: usize = digits.parse().ok()?;
    let start = colon + 1;
    let end = start.checked_add(len)?;
    Some((bytes.get(start..end)?, end))
}
