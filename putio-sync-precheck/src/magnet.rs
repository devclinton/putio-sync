//! The info hash of a magnet link (moved from putio-sync-arr for the watch folders)

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
