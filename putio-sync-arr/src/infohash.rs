//! The BitTorrent info hash of a magnet link or a .torrent file: the id Radarr and
//! Sonarr compute themselves and look items up by (research R4)

#[cfg(test)]
use sha1::{Digest, Sha1};

/// The `btih` hash of a magnet link as 40 lowercase hex characters, and its `dn` name
/// when it has one. Accepts hex and base32 hashes; `None` when there is no valid one.
pub fn from_magnet(magnet: &str) -> Option<(String, Option<String>)> {
    let query = magnet.strip_prefix("magnet:?")?;
    let mut hash = None;
    let mut name = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "xt" if hash.is_none() => {
                let value = percent_decode(value);
                if let Some(h) = value
                    .get(..9)
                    .filter(|p| p.eq_ignore_ascii_case("urn:btih:"))
                    .and_then(|_| parse_btih(&value[9..]))
                {
                    hash = Some(h);
                }
            }
            "dn" if name.is_none() => {
                let decoded = percent_decode(&value.replace('+', " "));
                if !decoded.trim().is_empty() {
                    name = Some(decoded);
                }
            }
            _ => {}
        }
    }
    hash.map(|h| (h, name))
}

fn parse_btih(value: &str) -> Option<String> {
    match value.len() {
        40 if value.bytes().all(|b| b.is_ascii_hexdigit()) => Some(value.to_ascii_lowercase()),
        32 => base32_decode(value).map(|bytes| hex(&bytes)),
        _ => None,
    }
}

/// RFC 4648 base32 without padding, 32 characters → 20 bytes
fn base32_decode(value: &str) -> Option<Vec<u8>> {
    let mut bits: u64 = 0;
    let mut count = 0;
    let mut out = Vec::with_capacity(20);
    for c in value.bytes() {
        let v = match c.to_ascii_uppercase() {
            c @ b'A'..=b'Z' => c - b'A',
            c @ b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(v);
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    (out.len() == 20).then_some(out)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let pair = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = pair.and_then(|p| u8::from_str_radix(p, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// The info hash of a .torrent file and its `info.name`, now read by the pre-check crate
pub use putio_sync_precheck::metainfo::info_hash as from_torrent;

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";

    #[test]
    fn hex_magnet() {
        let magnet = format!(
            "magnet:?xt=urn:btih:{}&dn=Show.S01E01.1080p&tr=udp%3A%2F%2Ftracker",
            HEX.to_uppercase()
        );
        assert_eq!(
            from_magnet(&magnet),
            Some((HEX.to_string(), Some("Show.S01E01.1080p".to_string())))
        );
    }

    #[test]
    fn base32_magnet() {
        // The same 20 bytes as HEX, base32-encoded
        let magnet = "magnet:?xt=urn:btih:YEX6DQDLXISUVHOJ6UM3GNNKPQJWPKEK&dn=a%20b+c";
        assert_eq!(
            from_magnet(magnet),
            Some((HEX.to_string(), Some("a b c".to_string())))
        );
    }

    #[test]
    fn bad_magnets() {
        assert_eq!(from_magnet("magnet:?dn=x"), None);
        assert_eq!(from_magnet("magnet:?xt=urn:btih:1234"), None);
        assert_eq!(from_magnet("magnet:?xt=urn:btmh:1220abcd"), None);
        assert_eq!(from_magnet("http://example.com/a.torrent"), None);
        assert_eq!(
            from_magnet("magnet:?xt=urn:btih:ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ1"),
            None
        );
    }

    /// A .torrent built by hand; the expected hash is the SHA-1 of its info bytes
    #[test]
    fn torrent_file() {
        let info = b"d6:lengthi1024e4:name14:Movie.2024.mkv12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let mut torrent = Vec::new();
        torrent.extend_from_slice(
            b"d8:announce18:http://tracker/ann13:announce-listll18:http://tracker/annee4:info",
        );
        torrent.extend_from_slice(info);
        torrent.extend_from_slice(b"e");

        let expected = hex(&Sha1::digest(info));
        assert_eq!(
            from_torrent(&torrent),
            Some((expected, Some("Movie.2024.mkv".to_string())))
        );
    }

    #[test]
    fn bad_torrents() {
        assert_eq!(from_torrent(b""), None);
        assert_eq!(from_torrent(b"<html>not found</html>"), None);
        assert_eq!(from_torrent(b"d8:announce3:urle"), None);
        assert_eq!(from_torrent(b"d4:infoi3ee"), None);
        assert_eq!(from_torrent(b"d4:infod4:name"), None);
        assert_eq!(from_torrent(b"d99999999999999999999:x"), None);
        let deep = format!("d4:info{}", "l".repeat(200));
        assert_eq!(from_torrent(deep.as_bytes()), None);
    }
}
