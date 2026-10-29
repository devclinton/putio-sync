//! The BitTorrent info hash of a magnet link or a .torrent file: the id Radarr and
//! Sonarr compute themselves and look items up by (research R4)

#[cfg(test)]
use sha1::{Digest, Sha1};

/// The `btih` hash of a magnet link and its `dn` name, now read by the pre-check crate
pub use putio_sync_precheck::magnet::from_magnet;

/// The info hash of a .torrent file and its `info.name`, now read by the pre-check crate
pub use putio_sync_precheck::metainfo::info_hash as from_torrent;

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

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
