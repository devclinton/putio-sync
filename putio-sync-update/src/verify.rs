//! Signature and hash checks for release assets (research R6).
//!
//! A release's `SHA256SUMS` is signed with the project's minisign key. Every download is
//! checked against that list before it is decoded, and the binary it makes against the
//! list's `.bin` line, so a swap only ever installs bytes the release key vouched for.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// The release public key (`minisign -G`, MAINTAINING.md "MINISIGN_SECRET_KEY and
/// MINISIGN_PASSWORD"). Empty until the maintainer has made the key: every update then fails
/// at the signature check.
pub const RELEASE_PUBKEY: &str = "";

/// The key `SHA256SUMS` must be signed with. Debug builds take `PUTIO_SYNC_UPDATE_PUBKEY`,
/// so the quickstart's fake release can be signed with the test key in `tests/data/`.
pub fn pubkey() -> String {
    #[cfg(debug_assertions)]
    if let Ok(key) = std::env::var("PUTIO_SYNC_UPDATE_PUBKEY") {
        return key;
    }
    RELEASE_PUBKEY.to_string()
}

/// Which check refused the update; the label of `putio_sync_update_verify_failures_total`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// No signature, a bad one, another key's, or a list changed after signing
    Signature(String),
    /// A download that isn't in the list or doesn't match its line
    Download(String),
    /// The decoded binary doesn't match its `.bin` line
    Binary(String),
}

impl VerifyError {
    /// The metric label
    pub fn stage(&self) -> &'static str {
        match self {
            Self::Signature(_) => "signature",
            Self::Download(_) => "download",
            Self::Binary(_) => "binary",
        }
    }

    /// Count this failure
    pub fn record(&self) {
        putio_sync_telemetry::update::record_verify_failure(self.stage());
    }
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signature(e) => write!(f, "the release signature did not verify: {e}"),
            Self::Download(e) | Self::Binary(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// A parsed `SHA256SUMS`: file name to hash
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sums(HashMap<String, [u8; 32]>);

impl Sums {
    /// Parse `<hex>  <name>` and `<hex> *<name>` lines; anything else is skipped
    pub fn parse(text: &str) -> Self {
        let lines = text.lines().filter_map(|line| {
            let (hex, name) = line.trim_end().split_once(' ')?;
            let name = name.strip_prefix(' ').or(name.strip_prefix('*'))?;
            Some((name.to_string(), decode_hex(hex)?))
        });
        Self(lines.collect())
    }

    /// The hash listed for `name`
    pub fn get(&self, name: &str) -> Option<&[u8; 32]> {
        self.0.get(name)
    }

    /// `name` is listed
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Check `hash` against `name`'s line
    pub fn check_hash(&self, hash: &[u8; 32], name: &str) -> Result<(), String> {
        match self.get(name) {
            None => Err(format!("{name} is not in SHA256SUMS")),
            Some(want) if want != hash => Err(format!("{name} does not match SHA256SUMS")),
            Some(_) => Ok(()),
        }
    }

    /// Check `bytes` against `name`'s line
    pub fn check(&self, bytes: &[u8], name: &str) -> Result<(), String> {
        self.check_hash(&Sha256::digest(bytes).into(), name)
    }
}

/// Verify `sums` against its `.minisig` and `pubkey` (the public key file or its base64
/// line), then parse it
pub fn verify_list(sums: &[u8], minisig: &str, pubkey: &str) -> Result<Sums, VerifyError> {
    let key_line = pubkey
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .unwrap_or_default();
    if key_line.is_empty() {
        return Err(VerifyError::Signature(
            "this build has no release key".into(),
        ));
    }
    let key = minisign_verify::PublicKey::from_base64(key_line)
        .map_err(|e| VerifyError::Signature(format!("bad release key: {e}")))?;
    let signature = minisign_verify::Signature::decode(minisig)
        .map_err(|e| VerifyError::Signature(e.to_string()))?;
    key.verify(sums, &signature, false)
        .map_err(|e| VerifyError::Signature(e.to_string()))?;
    let text = std::str::from_utf8(sums)
        .map_err(|_| VerifyError::Signature("SHA256SUMS is not text".into()))?;
    Ok(Sums::parse(text))
}

fn decode_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Lowercase hex, as `sha256sum` writes it
pub fn hex(hash: &[u8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const TEST_PUBKEY: &str = include_str!("../tests/data/minisign-test.pub");

    /// Sign `data` with the test key (slow: the key is password-protected, so it is read once)
    pub fn sign(data: &[u8]) -> String {
        use std::sync::OnceLock;
        static KEY: OnceLock<minisign::SecretKey> = OnceLock::new();
        let key = KEY.get_or_init(|| {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/minisign-test.key");
            minisign::SecretKey::from_file(path, Some("putio-sync-test".into())).unwrap()
        });
        minisign::sign(None, key, data, Some("putio-sync test"), None)
            .unwrap()
            .into_string()
    }

    /// A `SHA256SUMS` line for `bytes`
    pub fn line(bytes: &[u8], name: &str) -> String {
        format!("{}  {name}\n", hex(&Sha256::digest(bytes)))
    }

    #[test]
    fn parses_both_line_styles() {
        let text = format!(
            "{}{} *b.xz\nnot a line\n{}  \n",
            line(b"a", "a.xz"),
            hex(&Sha256::digest(b"b")),
            "0".repeat(64),
        );
        let sums = Sums::parse(&text);
        assert!(sums.check(b"a", "a.xz").is_ok());
        assert!(sums.check(b"b", "b.xz").is_ok());
        assert_eq!(sums.0.len(), 2);
    }

    #[test]
    fn check_needs_the_name_and_the_hash() {
        let sums = Sums::parse(&line(b"a", "a.xz"));
        assert_eq!(
            sums.check(b"a", "b.xz"),
            Err("b.xz is not in SHA256SUMS".into())
        );
        assert_eq!(
            sums.check(b"x", "a.xz"),
            Err("a.xz does not match SHA256SUMS".into())
        );
    }

    #[test]
    fn verify_list_accepts_the_test_key() {
        let list = line(b"a", "a.xz");
        let sums = verify_list(list.as_bytes(), &sign(list.as_bytes()), TEST_PUBKEY).unwrap();
        assert!(sums.check(b"a", "a.xz").is_ok());
    }

    #[test]
    fn verify_list_refuses_bad_signatures() {
        let list = line(b"a", "a.xz");
        let good = sign(list.as_bytes());
        let refused = |sums: &[u8], sig: &str, key: &str| {
            matches!(verify_list(sums, sig, key), Err(VerifyError::Signature(_)))
        };
        // missing
        assert!(refused(list.as_bytes(), "", TEST_PUBKEY));
        // changed after signing
        let changed = line(b"b", "a.xz");
        assert!(refused(changed.as_bytes(), &good, TEST_PUBKEY));
        // another key's
        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let theirs = minisign::sign(None, &other.sk, list.as_bytes(), None, None)
            .unwrap()
            .into_string();
        assert!(refused(list.as_bytes(), &theirs, TEST_PUBKEY));
        // no key in this build
        assert!(refused(list.as_bytes(), &good, ""));
    }

    #[test]
    fn the_test_key_is_not_the_release_key() {
        assert!(!RELEASE_PUBKEY.contains("RWSXo7X1t7J3bXkWaVrzgz"));
    }

    /// `cargo test --release` fails until the maintainer has put the release key in
    #[test]
    #[cfg(not(debug_assertions))]
    fn release_builds_have_the_release_key() {
        assert!(
            !RELEASE_PUBKEY.is_empty(),
            "set RELEASE_PUBKEY (MAINTAINING.md)"
        );
    }
}
