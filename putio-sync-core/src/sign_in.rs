//! `[sign_in]`: ways into the web UI and API besides the API token (feature 019). A
//! username and password (a form, or HTTP Basic) and OAuth sign-in with Google, GitHub or
//! Discord, limited to listed accounts. Validation, masking and the allow-list live here;
//! the sign-in itself is in `putio-sync-api`.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
pub use crate::hooks::MASK;

/// Default `session_hours`: a week
pub const DEFAULT_SESSION_HOURS: u32 = 168;

/// Longest `session_hours`: a year
pub const MAX_SESSION_HOURS: u32 = 8760;

/// The OAuth providers, in the order the sign-in page shows them
pub const PROVIDERS: [Provider; 3] = [Provider::Google, Provider::Github, Provider::Discord];

/// An OAuth provider
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Google accounts
    Google,
    /// GitHub accounts
    Github,
    /// Discord accounts
    Discord,
}

impl Provider {
    /// The config key and URL part: `google`, `github`, `discord`
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Github => "github",
            Provider::Discord => "discord",
        }
    }

    /// The name people know it by
    pub fn title(self) -> &'static str {
        match self {
            Provider::Google => "Google",
            Provider::Github => "GitHub",
            Provider::Discord => "Discord",
        }
    }

    /// The provider named `s`
    pub fn parse(s: &str) -> Option<Provider> {
        PROVIDERS.into_iter().find(|p| p.as_str() == s)
    }
}

fn default_session_hours() -> u32 {
    DEFAULT_SESSION_HOURS
}

fn is_default_hours(hours: &u32) -> bool {
    *hours == DEFAULT_SESSION_HOURS
}

/// The `[sign_in]` section
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SignInConfig {
    /// The address people open, e.g. `https://sync.example.com`, for OAuth redirect URLs
    /// behind a reverse proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    /// How long a sign-in lasts
    #[serde(
        default = "default_session_hours",
        skip_serializing_if = "is_default_hours"
    )]
    pub session_hours: u32,
    /// Username and password
    #[serde(skip_serializing_if = "PasswordConfig::is_empty")]
    pub password: PasswordConfig,
    /// Sign in with Google
    #[serde(skip_serializing_if = "ProviderConfig::is_empty")]
    pub google: ProviderConfig,
    /// Sign in with GitHub
    #[serde(skip_serializing_if = "ProviderConfig::is_empty")]
    pub github: ProviderConfig,
    /// Sign in with Discord
    #[serde(skip_serializing_if = "ProviderConfig::is_empty")]
    pub discord: ProviderConfig,
}

impl Default for SignInConfig {
    fn default() -> Self {
        SignInConfig {
            public_url: None,
            session_hours: DEFAULT_SESSION_HOURS,
            password: PasswordConfig::default(),
            google: ProviderConfig::default(),
            github: ProviderConfig::default(),
            discord: ProviderConfig::default(),
        }
    }
}

/// `[sign_in.password]`
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PasswordConfig {
    /// The username to sign in with
    pub username: String,
    /// An argon2id PHC string (`$argon2id$v=19$…`); never the password itself
    pub password_hash: String,
}

impl PasswordConfig {
    /// Nothing set
    pub fn is_empty(&self) -> bool {
        self.username.is_empty() && self.password_hash.is_empty()
    }

    /// Whether password sign-in is on: a username and a hash
    pub fn is_set(&self) -> bool {
        !self.username.trim().is_empty() && !self.password_hash.is_empty()
    }
}

/// `[sign_in.google]`, `[sign_in.github]`, `[sign_in.discord]`
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// Offer this provider on the sign-in page
    pub enabled: bool,
    /// The OAuth app's client ID
    pub client_id: String,
    /// The OAuth app's client secret, shown as [`MASK`]
    pub client_secret: String,
    /// Who may sign in: a verified email, `@example.com` for a whole domain, or the
    /// account name (GitHub login, Discord username)
    pub allowed_users: Vec<String>,
}

impl ProviderConfig {
    /// Nothing set
    pub fn is_empty(&self) -> bool {
        *self == ProviderConfig::default()
    }
}

/// Who a provider says signed in
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Account {
    /// GitHub login, Discord username; empty for Google
    pub name: String,
    /// Emails the provider has verified
    pub emails: Vec<String>,
    /// For the top bar
    pub display_name: String,
}

impl Account {
    /// The login a session is kept under: the account name, else the first verified email
    pub fn login(&self) -> String {
        if self.name.is_empty() {
            self.emails.first().cloned().unwrap_or_default()
        } else {
            self.name.clone()
        }
    }
}

fn bad(field: &str, message: &str) -> CoreResult<()> {
    Err(CoreError::ConfigError(format!(
        "sign_in.{field}: {message}"
    )))
}

/// `true` for an entry `allowed_users` can hold
fn valid_entry(entry: &str) -> bool {
    let entry = entry.trim();
    !entry.is_empty() && !entry.contains(char::is_whitespace) && entry != "@"
}

/// Whether `value` is the `entry` an allow-list holds, ignoring case
fn same(entry: &str, value: &str) -> bool {
    !value.is_empty() && entry.trim().eq_ignore_ascii_case(value.trim())
}

impl SignInConfig {
    /// Nothing set, so the section is left out of the file
    pub fn is_default(&self) -> bool {
        *self == SignInConfig::default()
    }

    /// The settings of `provider`
    pub fn provider(&self, provider: Provider) -> &ProviderConfig {
        match provider {
            Provider::Google => &self.google,
            Provider::Github => &self.github,
            Provider::Discord => &self.discord,
        }
    }

    /// The settings of `provider`, to change
    pub fn provider_mut(&mut self, provider: Provider) -> &mut ProviderConfig {
        match provider {
            Provider::Google => &mut self.google,
            Provider::Github => &mut self.github,
            Provider::Discord => &mut self.discord,
        }
    }

    /// The providers that are on
    pub fn enabled_providers(&self) -> Vec<Provider> {
        PROVIDERS
            .into_iter()
            .filter(|p| self.provider(*p).enabled)
            .collect()
    }

    /// Whether any way in besides the token is on
    pub fn any(&self) -> bool {
        self.password.is_set() || !self.enabled_providers().is_empty()
    }

    /// Check the section. Errors name the field.
    pub fn validate(&self) -> CoreResult<()> {
        if let Some(url) = &self.public_url {
            if let Err(why) = check_public_url(url) {
                return bad("public_url", &why);
            }
        }
        if self.session_hours == 0 || self.session_hours > MAX_SESSION_HOURS {
            return bad(
                "session_hours",
                &format!("must be between 1 and {MAX_SESSION_HOURS}"),
            );
        }
        let password = &self.password;
        if !password.password_hash.is_empty() {
            if !password.password_hash.starts_with("$argon2") {
                return bad(
                    "password.password_hash",
                    "must be an argon2 hash; set the password in Settings › Sign-in",
                );
            }
            if password.username.trim().is_empty() {
                return bad("password.username", "a username is needed with a password");
            }
        }
        if password.username.contains(':') {
            return bad("password.username", "must not contain ':'");
        }
        for provider in PROVIDERS {
            let config = self.provider(provider);
            let key = provider.as_str();
            if let Some(entry) = config.allowed_users.iter().find(|e| !valid_entry(e)) {
                return bad(
                    &format!("{key}.allowed_users"),
                    &format!("\"{entry}\" is not an email, @domain or account name"),
                );
            }
            if !config.enabled {
                continue;
            }
            if config.client_id.trim().is_empty() {
                return bad(&format!("{key}.client_id"), "needed when sign-in is on");
            }
            if config.client_secret.trim().is_empty() {
                return bad(&format!("{key}.client_secret"), "needed when sign-in is on");
            }
            if config.allowed_users.is_empty() {
                return bad(
                    &format!("{key}.allowed_users"),
                    "list at least one email, @domain or account name",
                );
            }
        }
        Ok(())
    }

    /// Whether `account` may sign in with `provider`: an entry equals its account name or
    /// one of its verified emails, or is `@domain` and a verified email ends with it
    pub fn allows(&self, provider: Provider, account: &Account) -> bool {
        let config = self.provider(provider);
        config.enabled
            && config.allowed_users.iter().any(|entry| {
                let entry = entry.trim();
                if entry.starts_with('@') {
                    let domain = entry.to_lowercase();
                    return account
                        .emails
                        .iter()
                        .any(|email| email.trim().to_lowercase().ends_with(&domain));
                }
                same(entry, &account.name) || account.emails.iter().any(|e| same(entry, e))
            })
    }

    /// Whether the account kept in a session as `login` is still allowed
    pub fn allows_login(&self, provider: Provider, login: &str) -> bool {
        let account = if login.contains('@') {
            Account {
                emails: vec![login.to_string()],
                ..Account::default()
            }
        } else {
            Account {
                name: login.to_string(),
                ..Account::default()
            }
        };
        self.allows(provider, &account)
    }

    /// `public_url` without a trailing `/`
    pub fn base_url(&self) -> Option<&str> {
        self.public_url
            .as_deref()
            .map(|u| u.trim().trim_end_matches('/'))
            .filter(|u| !u.is_empty())
    }

    /// A copy with the client secrets replaced by [`MASK`] when set
    pub fn masked(&self) -> Self {
        let mut c = self.clone();
        for provider in PROVIDERS {
            let p = c.provider_mut(provider);
            if !p.client_secret.is_empty() {
                p.client_secret = MASK.to_string();
            }
        }
        c
    }

    /// Put the stored secrets back where `self` carries [`MASK`]
    pub fn unmasked(mut self, current: &SignInConfig) -> Self {
        for provider in PROVIDERS {
            if self.provider(provider).client_secret == MASK {
                self.provider_mut(provider).client_secret =
                    current.provider(provider).client_secret.clone();
            }
        }
        self
    }

    /// The client secrets and the password hash, for log redaction
    pub fn secret_values(&self) -> Vec<String> {
        let mut out = vec![self.password.password_hash.clone()];
        out.extend(PROVIDERS.map(|p| self.provider(p).client_secret.clone()));
        out.retain(|v| !v.trim().is_empty());
        out
    }
}

/// Why `url` can't be a public URL, if it can't
fn check_public_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| format!("\"{url}\" must start with http:// or https://"))?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) || host.contains('@') {
        return Err(format!("\"{url}\" has no host name"));
    }
    if url.contains(['?', '#']) {
        return Err(format!("\"{url}\" must not have a query or #fragment"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github(allowed: &[&str]) -> SignInConfig {
        SignInConfig {
            github: ProviderConfig {
                enabled: true,
                client_id: "id".into(),
                client_secret: "secret".into(),
                allowed_users: allowed.iter().map(|s| s.to_string()).collect(),
            },
            ..SignInConfig::default()
        }
    }

    fn account(name: &str, emails: &[&str]) -> Account {
        Account {
            name: name.into(),
            emails: emails.iter().map(|s| s.to_string()).collect(),
            display_name: String::new(),
        }
    }

    #[test]
    fn the_default_is_empty_and_valid() {
        let config = SignInConfig::default();
        assert!(!config.any());
        config.validate().unwrap();
        let text = toml::to_string(&config).unwrap();
        assert_eq!(text.trim(), "");
    }

    #[test]
    fn the_allow_list_matches_names_emails_and_domains() {
        let config = github(&["Octocat", "alice@example.com", "@corp.example"]);
        assert!(config.allows(Provider::Github, &account("octocat", &[])));
        assert!(config.allows(Provider::Github, &account("x", &["ALICE@example.com"])));
        assert!(config.allows(Provider::Github, &account("x", &["bob@corp.example"])));
        assert!(!config.allows(
            Provider::Github,
            &account("x", &["bob@notcorp.example.com"])
        ));
        assert!(!config.allows(Provider::Github, &account("x", &["bob@xcorp.example2"])));
        assert!(!config.allows(Provider::Github, &account("", &[])));
        // Another provider, or this one turned off, lets nobody in
        assert!(!config.allows(Provider::Google, &account("octocat", &[])));
        let mut off = config.clone();
        off.github.enabled = false;
        assert!(!off.allows(Provider::Github, &account("octocat", &[])));
        assert!(config.allows_login(Provider::Github, "octocat"));
        assert!(config.allows_login(Provider::Github, "bob@corp.example"));
        assert!(!config.allows_login(Provider::Github, "mallory"));
    }

    #[test]
    fn validation_names_the_field() {
        let err = |c: SignInConfig| c.validate().unwrap_err().to_string();
        let mut c = github(&["a"]);
        c.github.client_secret.clear();
        assert!(err(c).contains("sign_in.github.client_secret"));
        assert!(err(github(&[])).contains("sign_in.github.allowed_users"));
        assert!(err(github(&["two words"])).contains("\"two words\""));
        let mut c = github(&["a"]);
        c.public_url = Some("sync.example.com".into());
        assert!(err(c).contains("sign_in.public_url"));
        let mut c = github(&["a"]);
        c.public_url = Some("https://sync.example.com/?x=1".into());
        assert!(err(c).contains("query"));
        let mut c = SignInConfig::default();
        c.password.password_hash = "plain".into();
        c.password.username = "admin".into();
        assert!(err(c).contains("argon2"));
        let mut c = SignInConfig::default();
        c.password.password_hash = "$argon2id$v=19$m=19456,t=2,p=1$x$y".into();
        assert!(err(c).contains("password.username"));
        let c = SignInConfig {
            session_hours: 0,
            ..SignInConfig::default()
        };
        assert!(err(c).contains("session_hours"));
        let mut c = github(&["a"]);
        c.public_url = Some("https://sync.example.com/putio/".into());
        c.validate().unwrap();
        assert_eq!(c.base_url(), Some("https://sync.example.com/putio"));
    }

    #[test]
    fn secrets_are_masked_and_kept() {
        let stored = github(&["a"]);
        let shown = stored.masked();
        assert_eq!(shown.github.client_secret, MASK);
        assert_eq!(shown.google.client_secret, "");
        assert_eq!(shown.clone().unmasked(&stored), stored);
        assert_eq!(stored.secret_values(), vec!["secret".to_string()]);
    }

    #[test]
    fn the_section_reads_from_toml() {
        let config: SignInConfig = toml::from_str(
            r#"
            public_url = "https://sync.example.com"
            [password]
            username = "admin"
            password_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
            [discord]
            enabled = true
            client_id = "1"
            client_secret = "s"
            allowed_users = ["someone"]
            "#,
        )
        .unwrap();
        config.validate().unwrap();
        assert_eq!(config.session_hours, DEFAULT_SESSION_HOURS);
        assert!(config.password.is_set());
        assert_eq!(config.enabled_providers(), vec![Provider::Discord]);
        assert_eq!(Provider::parse("discord"), Some(Provider::Discord));
        assert_eq!(Provider::parse("gitlab"), None);
    }
}
