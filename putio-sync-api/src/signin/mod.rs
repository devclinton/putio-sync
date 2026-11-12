//! Sign-in options (feature 019): passwords, sessions and OAuth state
//!
//! - Passwords are argon2id hashes. A verified HTTP Basic credential is remembered for a
//!   few minutes, so scripts don't pay for a hash on every request.
//! - A session is an HMAC-signed cookie (see [`session`]). The key is `session.key` next
//!   to the config file, made on first sign-in, so sessions survive a restart. Without a
//!   config file it lives in memory.
//! - An OAuth sign-in in progress is kept in memory for [`FLOW_LIFETIME`] (see [`oauth`]).

pub mod oauth;
pub mod session;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::http::HeaderMap;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use putio_sync_core::sign_in::{PasswordConfig, Provider, SignInConfig};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::warn;

/// How long an OAuth sign-in may take between the redirect and the callback
pub const FLOW_LIFETIME: Duration = Duration::from_secs(600);

/// OAuth sign-ins in progress at once; the oldest is dropped past this
const MAX_FLOWS: usize = 100;

/// How long a verified Basic credential is remembered
const BASIC_CACHE_TTL: Duration = Duration::from_secs(300);

/// Basic credentials remembered at once
const MAX_BASIC_CACHE: usize = 16;

/// The session key file, next to the config file
pub const KEY_FILE: &str = "session.key";

/// `n` random bytes from the OS
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::getrandom(&mut out).expect("the OS random number generator failed");
    out
}

/// 32 random bytes as base64url: OAuth state, PKCE verifiers, browser bindings
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

/// An argon2id hash of `password` as a PHC string. Slow on purpose: call it off the
/// async runtime (`spawn_blocking`).
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()).map_err(|e| e.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| e.to_string())
}

/// Whether `password` matches the PHC string `hash`. Slow on purpose, like
/// [`hash_password`].
pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

/// A short fingerprint of the password hash: sessions and remembered Basic credentials
/// carry it, so a new password ends them
pub fn password_stamp(hash: &str) -> String {
    let digest = Sha256::digest(hash.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// An OAuth sign-in between the redirect to the provider and its callback
#[derive(Debug, Clone)]
pub struct Flow {
    pub provider: Provider,
    /// PKCE code verifier
    pub verifier: String,
    /// SHA-256 of the browser's `putio_sync_oauth` cookie
    pub binding: [u8; 32],
    /// The redirect URL sent to the provider, sent again in the code exchange
    pub redirect_uri: String,
    /// Where in the UI to land afterwards (`#/…`)
    pub return_to: String,
    created: Instant,
}

/// Sign-in state shared by the middleware and the routes
pub struct SignIn {
    key_path: Option<PathBuf>,
    key: RwLock<Option<[u8; 32]>>,
    flows: Mutex<HashMap<String, Flow>>,
    basic: Mutex<Vec<([u8; 32], Instant)>>,
    /// Per-process salt for remembered Basic credentials
    basic_salt: [u8; 32],
    /// Where the OAuth providers are; `None` is the real ones. Tests point it at a mock
    /// server.
    pub endpoints: RwLock<Option<String>>,
    /// HTTP client for the code exchange and user info
    pub http: reqwest::Client,
}

impl SignIn {
    /// Sign-in state keeping its key in `config_dir` (memory only when `None`)
    pub fn new(config_dir: Option<PathBuf>) -> SignIn {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("putio-sync/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        SignIn {
            key_path: config_dir.map(|dir| dir.join(KEY_FILE)),
            key: RwLock::new(None),
            flows: Mutex::new(HashMap::new()),
            basic: Mutex::new(Vec::new()),
            basic_salt: random_bytes(),
            endpoints: RwLock::new(None),
            http,
        }
    }

    /// The session key, read from its file the first time. `None` before any sign-in.
    pub fn key(&self) -> Option<[u8; 32]> {
        if let Some(key) = *self.key.read().unwrap_or_else(|e| e.into_inner()) {
            return Some(key);
        }
        let key = read_key(self.key_path.as_ref()?)?;
        *self.key.write().unwrap_or_else(|e| e.into_inner()) = Some(key);
        Some(key)
    }

    /// The session key, made (and saved) now if there is none
    pub fn key_or_create(&self) -> [u8; 32] {
        if let Some(key) = self.key() {
            return key;
        }
        self.new_key()
    }

    /// Make and save a new key, which ends every session ("Sign out everywhere")
    pub fn new_key(&self) -> [u8; 32] {
        let key = random_bytes::<32>();
        if let Some(path) = &self.key_path {
            if let Err(e) = write_key(path, &key) {
                warn!(path = %path.display(), error = %e, "Can't save the session key; sessions end when putio-sync stops");
            }
        }
        *self.key.write().unwrap_or_else(|e| e.into_inner()) = Some(key);
        self.basic.lock().unwrap_or_else(|e| e.into_inner()).clear();
        key
    }

    /// Keep a new OAuth sign-in under `state`
    pub fn start_flow(
        &self,
        state: String,
        provider: Provider,
        verifier: String,
        binding: [u8; 32],
        redirect_uri: String,
        return_to: String,
    ) {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        flows.retain(|_, f| f.created.elapsed() < FLOW_LIFETIME);
        if flows.len() >= MAX_FLOWS {
            if let Some(oldest) = flows
                .iter()
                .min_by_key(|(_, f)| f.created)
                .map(|(k, _)| k.clone())
            {
                flows.remove(&oldest);
            }
        }
        flows.insert(
            state,
            Flow {
                provider,
                verifier,
                binding,
                redirect_uri,
                return_to,
                created: Instant::now(),
            },
        );
    }

    /// Take the OAuth sign-in kept under `state`; each works once and only while fresh
    pub fn take_flow(&self, state: &str) -> Option<Flow> {
        let flow = self
            .flows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(state)?;
        (flow.created.elapsed() < FLOW_LIFETIME).then_some(flow)
    }

    fn basic_digest(&self, header: &str, hash: &str) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(self.basic_salt);
        digest.update(header.as_bytes());
        digest.update([0]);
        digest.update(hash.as_bytes());
        digest.finalize().into()
    }

    /// Whether the Basic credential `header` was verified against `hash` lately
    pub fn basic_known(&self, header: &str, hash: &str) -> bool {
        let digest = self.basic_digest(header, hash);
        let mut cache = self.basic.lock().unwrap_or_else(|e| e.into_inner());
        cache.retain(|(_, at)| at.elapsed() < BASIC_CACHE_TTL);
        cache.iter().any(|(d, _)| *d == digest)
    }

    /// Remember that the Basic credential `header` matched `hash`
    pub fn remember_basic(&self, header: &str, hash: &str) {
        let digest = self.basic_digest(header, hash);
        let mut cache = self.basic.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= MAX_BASIC_CACHE {
            cache.remove(0);
        }
        cache.push((digest, Instant::now()));
    }
}

/// A session the request's cookie carries that the config still allows, with the cookie
/// value (for its CSRF token)
///
/// A password session needs the same username and password stamp; a provider session
/// needs the provider on and the login still on its allow-list.
pub fn current_session(
    sign_in: &SignIn,
    config: &SignInConfig,
    headers: &HeaderMap,
) -> Option<(session::Session, String, [u8; 32])> {
    let value = session::cookie(headers, session::COOKIE_NAME)?;
    let key = sign_in.key()?;
    let found = session::verify(&key, value)?;
    let still_allowed = if found.how == "password" {
        config.password.is_set()
            && found.login == config.password.username
            && found.stamp == password_stamp(&config.password.password_hash)
    } else {
        Provider::parse(&found.how).is_some_and(|p| config.allows_login(p, &found.login))
    };
    still_allowed.then(|| (found, value.to_string(), key))
}

/// The username and password in an `Authorization: Basic …` value
pub fn basic_credentials(header: &str) -> Option<(String, String)> {
    let encoded = header.trim().strip_prefix("Basic ")?.trim();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

/// Whether the Basic `header` carries the configured username and password. Verified
/// credentials are remembered for a few minutes; the hash runs off the async runtime.
pub async fn basic_matches(sign_in: &SignIn, header: &str, password: &PasswordConfig) -> bool {
    let Some((user, pass)) = basic_credentials(header) else {
        return false;
    };
    if user != password.username {
        return false;
    }
    let hash = password.password_hash.clone();
    if sign_in.basic_known(header, &hash) {
        return true;
    }
    let check = hash.clone();
    let ok = tokio::task::spawn_blocking(move || verify_password(&pass, &check))
        .await
        .unwrap_or(false);
    if ok {
        sign_in.remember_basic(header, &hash);
    }
    ok
}

/// Where this server is reached from a browser: `public_url`, else the scheme and host
/// the proxy or the browser sent
pub fn external_base(config: &SignInConfig, headers: &HeaderMap) -> String {
    if let Some(base) = config.base_url() {
        return base.to_string();
    }
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let scheme = header("x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    let host = header("x-forwarded-host")
        .or_else(|| header("host"))
        .unwrap_or_else(|| "localhost".to_string());
    format!("{scheme}://{host}")
}

/// The redirect URL to register with `provider`
pub fn redirect_url(config: &SignInConfig, headers: &HeaderMap, provider: Provider) -> String {
    format!(
        "{}/auth/oauth/{}/callback",
        external_base(config, headers),
        provider.as_str()
    )
}

fn read_key(path: &PathBuf) -> Option<[u8; 32]> {
    let text = std::fs::read_to_string(path).ok()?;
    let bytes = URL_SAFE_NO_PAD.decode(text.trim()).ok()?;
    bytes.try_into().ok()
}

fn write_key(path: &PathBuf, key: &[u8; 32]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("key.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(URL_SAFE_NO_PAD.encode(key).as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hash_and_verify() {
        let hash = hash_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("correct horse", "not a hash"));
        assert_ne!(password_stamp(&hash), password_stamp("other"));
    }

    #[test]
    fn the_key_is_saved_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let first = SignIn::new(Some(dir.path().to_path_buf()));
        assert_eq!(first.key(), None);
        let key = first.key_or_create();
        let again = SignIn::new(Some(dir.path().to_path_buf()));
        assert_eq!(again.key(), Some(key));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_ne!(again.new_key(), key);
    }

    #[test]
    fn flows_work_once() {
        let sign_in = SignIn::new(None);
        sign_in.start_flow(
            "s".into(),
            Provider::Github,
            "v".into(),
            [1; 32],
            "r".into(),
            "#/status".into(),
        );
        assert_eq!(sign_in.take_flow("s").unwrap().verifier, "v");
        assert!(sign_in.take_flow("s").is_none());
        assert!(sign_in.take_flow("other").is_none());
    }

    #[test]
    fn basic_credentials_are_remembered_per_hash() {
        let sign_in = SignIn::new(None);
        assert!(!sign_in.basic_known("Basic abc", "h1"));
        sign_in.remember_basic("Basic abc", "h1");
        assert!(sign_in.basic_known("Basic abc", "h1"));
        assert!(!sign_in.basic_known("Basic abc", "h2"));
        assert!(!sign_in.basic_known("Basic abd", "h1"));
    }
}
