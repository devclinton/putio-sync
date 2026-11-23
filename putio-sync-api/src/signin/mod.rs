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
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::warn;

/// How long an OAuth sign-in may take between the redirect and the callback
pub const FLOW_LIFETIME: Duration = Duration::from_secs(600);

/// OAuth sign-ins in progress at once; the oldest is dropped past this
const MAX_FLOWS: usize = 100;
/// Pending sign-ins one client address may hold; its oldest goes first, so one client can't
/// push another's sign-in out of the table
const MAX_FLOWS_PER_IP: usize = 5;

/// How long a verified Basic credential is remembered
const BASIC_CACHE_TTL: Duration = Duration::from_secs(300);

/// Basic credentials remembered at once
const MAX_BASIC_CACHE: usize = 16;

/// Password hashes that may run at once. Each argon2id check takes about 19 MiB and a
/// lot of CPU, so a burst of sign-ins must not all start together.
pub const HASH_PERMITS: usize = 2;

/// The session key file, next to the config file
pub const KEY_FILE: &str = "session.key";

/// Next to the key: the signatures of sessions that signed out, with their expiry, one
/// `signature expiry` pair per line. Sessions are stateless, so this is what makes
/// signing out end the cookie on the server too.
pub const REVOKED_FILE: &str = "session.revoked";

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
    /// The client address (its /64 for IPv6) that started it
    client: std::net::IpAddr,
}

/// Sign-in state shared by the middleware and the routes
pub struct SignIn {
    key_path: Option<PathBuf>,
    key: RwLock<Option<[u8; 32]>>,
    revoked_path: Option<PathBuf>,
    /// `(signature, expiry in Unix seconds)` of signed-out sessions
    revoked: Mutex<Vec<(String, u64)>>,
    flows: Mutex<HashMap<String, Flow>>,
    basic: Mutex<Vec<([u8; 32], Instant)>>,
    /// Per-process salt for remembered Basic credentials
    basic_salt: [u8; 32],
    /// Where the OAuth providers are; `None` is the real ones. Tests point it at a mock
    /// server.
    pub endpoints: RwLock<Option<String>>,
    /// HTTP client for the code exchange and user info
    pub http: reqwest::Client,
    /// Bounds the password hashes running at once
    hash_permits: Arc<tokio::sync::Semaphore>,
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
            revoked_path: config_dir.as_ref().map(|dir| dir.join(REVOKED_FILE)),
            revoked: Mutex::new(
                config_dir
                    .as_ref()
                    .map(|dir| read_revoked(&dir.join(REVOKED_FILE)))
                    .unwrap_or_default(),
            ),
            key_path: config_dir.map(|dir| dir.join(KEY_FILE)),
            key: RwLock::new(None),
            flows: Mutex::new(HashMap::new()),
            basic: Mutex::new(Vec::new()),
            basic_salt: random_bytes(),
            endpoints: RwLock::new(None),
            http,
            hash_permits: Arc::new(tokio::sync::Semaphore::new(HASH_PERMITS)),
        }
    }

    /// A place to run one password hash, or `None` when [`HASH_PERMITS`] are running
    /// already (answer 429 then)
    pub fn try_hash_permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        self.hash_permits.clone().try_acquire_owned().ok()
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
        // The old key's sessions are dead already
        self.revoked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        if let Some(path) = &self.revoked_path {
            let _ = std::fs::remove_file(path);
        }
        key
    }

    /// End the session whose cookie value is `cookie` (its signature is refused from
    /// now on, until it would have expired anyway)
    pub fn revoke(&self, cookie: &str, expires: u64) {
        let Some((_, tag)) = cookie.split_once('.') else {
            return;
        };
        let mut revoked = self.revoked.lock().unwrap_or_else(|e| e.into_inner());
        let now = unix_now();
        revoked.retain(|(_, exp)| *exp > now);
        revoked.push((tag.to_string(), expires));
        if let Some(path) = &self.revoked_path {
            if let Err(e) = write_revoked(path, &revoked) {
                warn!(path = %path.display(), error = %e, "Can't save the signed-out sessions; they stay signed out until putio-sync stops");
            }
        }
    }

    /// Whether the session with this cookie value has signed out
    pub fn is_revoked(&self, cookie: &str) -> bool {
        let Some((_, tag)) = cookie.split_once('.') else {
            return false;
        };
        self.revoked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|(t, _)| t == tag)
    }

    /// Keep a new OAuth sign-in under `state`, for the client address that started it
    #[allow(clippy::too_many_arguments)]
    pub fn start_flow(
        &self,
        state: String,
        provider: Provider,
        verifier: String,
        binding: [u8; 32],
        redirect_uri: String,
        return_to: String,
        client: std::net::IpAddr,
    ) {
        let client = crate::middleware::ratelimit::bucket(client);
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        flows.retain(|_, f| f.created.elapsed() < FLOW_LIFETIME);
        // This client's oldest sign-ins make room for its new one
        while flows.values().filter(|f| f.client == client).count() >= MAX_FLOWS_PER_IP {
            let Some(oldest) = flows
                .iter()
                .filter(|(_, f)| f.client == client)
                .min_by_key(|(_, f)| f.created)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            flows.remove(&oldest);
        }
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
                client,
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
    if sign_in.is_revoked(value) {
        return None;
    }
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

/// The answer to a Basic credential
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasicCheck {
    /// The username and password are right
    Match,
    /// Wrong username or password
    NoMatch,
    /// Too many password hashes are running; nothing was checked
    Busy,
}

/// Whether two strings are equal, compared in constant time
fn same(a: &str, b: &str) -> bool {
    crate::middleware::auth::token_matches(a, b)
}

/// A valid hash nobody has the password for, verified when the username is wrong so a
/// wrong username takes as long as a wrong password. Made once, on first use.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password(&random_token()).unwrap_or_else(|_| "not a hash".to_string()))
}

/// Verify `pass` against `hash` on the blocking pool, holding `permit` until done. With a
/// wrong username the check runs against a dummy hash and never matches.
pub async fn verify_blocking(
    pass: String,
    hash: String,
    user_ok: bool,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> bool {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let target = if user_ok { hash.as_str() } else { dummy_hash() };
        verify_password(&pass, target) && user_ok
    })
    .await
    .unwrap_or(false)
}

/// Compare a username in constant time
pub fn username_matches(provided: &str, expected: &str) -> bool {
    same(provided, expected)
}

/// Whether the Basic `header` carries the configured username and password. Verified
/// credentials are remembered for a few minutes; the hash runs off the async runtime, at
/// most [`HASH_PERMITS`] at a time. A wrong username takes as long as a wrong password.
pub async fn basic_matches(
    sign_in: &SignIn,
    header: &str,
    password: &PasswordConfig,
) -> BasicCheck {
    let Some((user, pass)) = basic_credentials(header) else {
        return BasicCheck::NoMatch;
    };
    let user_ok = username_matches(&user, &password.username);
    let hash = password.password_hash.clone();
    if user_ok && sign_in.basic_known(header, &hash) {
        return BasicCheck::Match;
    }
    let Some(permit) = sign_in.try_hash_permit() else {
        return BasicCheck::Busy;
    };
    if verify_blocking(pass, hash.clone(), user_ok, permit).await {
        sign_in.remember_basic(header, &hash);
        BasicCheck::Match
    } else {
        BasicCheck::NoMatch
    }
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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_revoked(path: &PathBuf) -> Vec<(String, u64)> {
    let now = unix_now();
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (tag, exp) = line.split_once(' ')?;
            Some((tag.to_string(), exp.trim().parse::<u64>().ok()?))
        })
        .filter(|(_, exp)| *exp > now)
        .collect()
}

fn write_revoked(path: &PathBuf, revoked: &[(String, u64)]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("revoked.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    for (tag, exp) in revoked {
        writeln!(file, "{tag} {exp}")?;
    }
    file.sync_all()?;
    std::fs::rename(&tmp, path)
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
    fn a_signed_out_session_stays_out_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let sign_in = SignIn::new(Some(dir.path().to_path_buf()));
        let key = sign_in.key_or_create();
        let session = session::Session::new("password", "admin", "", 1, "s".into());
        let other = session::Session::new("password", "admin", "", 2, "s".into());
        let cookie = session::sign(&key, &session);
        let other_cookie = session::sign(&key, &other);
        assert!(!sign_in.is_revoked(&cookie));
        sign_in.revoke(&cookie, session.exp);
        assert!(sign_in.is_revoked(&cookie));
        assert!(!sign_in.is_revoked(&other_cookie));
        let again = SignIn::new(Some(dir.path().to_path_buf()));
        assert!(again.is_revoked(&cookie));
        assert!(!again.is_revoked(&other_cookie));
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
            "10.0.0.1".parse().unwrap(),
        );
        assert_eq!(sign_in.take_flow("s").unwrap().verifier, "v");
        assert!(sign_in.take_flow("s").is_none());
        assert!(sign_in.take_flow("other").is_none());
    }

    #[test]
    fn one_client_cannot_push_out_another_clients_sign_in() {
        let sign_in = SignIn::new(None);
        let start = |state: String, ip: &str| {
            sign_in.start_flow(
                state,
                Provider::Github,
                "v".into(),
                [1; 32],
                "r".into(),
                "#/".into(),
                ip.parse().unwrap(),
            );
        };
        start("owner".into(), "192.168.1.10");
        // A flood from another address, far past the global cap
        for i in 0..(MAX_FLOWS * 3) {
            start(format!("flood{i}"), "192.168.1.99");
        }
        // The owner's sign-in survived, the flooder keeps only its newest
        assert!(sign_in.take_flow("owner").is_some());
        assert!(sign_in.take_flow("flood0").is_none());
        let last = MAX_FLOWS * 3 - 1;
        for i in (last + 1 - MAX_FLOWS_PER_IP)..=last {
            assert!(sign_in.take_flow(&format!("flood{i}")).is_some(), "{i}");
        }
    }

    #[test]
    fn the_global_cap_still_holds() {
        let sign_in = SignIn::new(None);
        for i in 0..(MAX_FLOWS + 20) {
            let ip = format!("10.1.{}.{}", i / 200, i % 200 + 1);
            sign_in.start_flow(
                format!("s{i}"),
                Provider::Github,
                "v".into(),
                [1; 32],
                "r".into(),
                "#/".into(),
                ip.parse().unwrap(),
            );
        }
        assert_eq!(sign_in.flows.lock().unwrap().len(), MAX_FLOWS);
        assert!(sign_in.take_flow("s0").is_none());
        assert!(sign_in.take_flow(&format!("s{}", MAX_FLOWS + 19)).is_some());
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
