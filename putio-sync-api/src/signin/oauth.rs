//! OAuth sign-in with Google, GitHub and Discord: authorization code flow with PKCE
//!
//! 1. `GET /auth/oauth/:provider` keeps a [`super::Flow`] under a random `state`, sets a
//!    binding cookie, and redirects to [`authorize_url`].
//! 2. The provider sends the browser back to `/auth/oauth/:provider/callback`. The state
//!    must be known, fresh and unused, and the binding cookie must match, so a callback
//!    started in another browser fails.
//! 3. [`exchange`] trades the code (with the PKCE verifier) for an access token, and
//!    [`account`] reads who it belongs to. Only verified emails count.

use super::Flow;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use putio_sync_core::sign_in::{Account, Provider, ProviderConfig, SignInConfig};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Where a provider's OAuth endpoints are
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub user: String,
    /// GitHub only: the verified emails
    pub emails: Option<String>,
}

/// The real endpoints of `provider`, or `{base}/{provider}/…` on a test server
pub fn endpoints(provider: Provider, base: Option<&str>) -> Endpoints {
    if let Some(base) = base {
        let base = base.trim_end_matches('/');
        let p = provider.as_str();
        return Endpoints {
            authorize: format!("{base}/{p}/authorize"),
            token: format!("{base}/{p}/token"),
            user: format!("{base}/{p}/user"),
            emails: (provider == Provider::Github).then(|| format!("{base}/{p}/emails")),
        };
    }
    let (authorize, token, user, emails) = match provider {
        Provider::Google => (
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
            "https://openidconnect.googleapis.com/v1/userinfo",
            None,
        ),
        Provider::Github => (
            "https://github.com/login/oauth/authorize",
            "https://github.com/login/oauth/access_token",
            "https://api.github.com/user",
            Some("https://api.github.com/user/emails"),
        ),
        Provider::Discord => (
            "https://discord.com/oauth2/authorize",
            "https://discord.com/api/oauth2/token",
            "https://discord.com/api/users/@me",
            None,
        ),
    };
    Endpoints {
        authorize: authorize.into(),
        token: token.into(),
        user: user.into(),
        emails: emails.map(Into::into),
    }
}

/// The scopes asked for: enough for a name and verified emails, nothing more
pub fn scope(provider: Provider) -> &'static str {
    match provider {
        Provider::Google => "openid email profile",
        Provider::Github => "read:user user:email",
        Provider::Discord => "identify email",
    }
}

/// The PKCE S256 challenge for `verifier`
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The SHA-256 of a binding cookie value, as kept in a [`Flow`]
pub fn binding_digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// The URL that sends the browser to the provider
pub fn authorize_url(
    endpoints: &Endpoints,
    provider: Provider,
    config: &ProviderConfig,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
) -> String {
    let mut url = reqwest::Url::parse(&endpoints.authorize)
        .unwrap_or_else(|_| reqwest::Url::parse("http://invalid/").expect("a valid URL"));
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", scope(provider))
            .append_pair("state", state)
            .append_pair("code_challenge", &challenge(verifier))
            .append_pair("code_challenge_method", "S256");
        if provider == Provider::Google {
            query.append_pair("prompt", "select_account");
        }
    }
    url.into()
}

/// Why a sign-in with a provider failed after the browser came back
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The provider sent an `error` instead of a code
    Denied,
    /// Unknown, expired or reused state, or another browser
    State,
    /// The token or user call failed
    Exchange(String),
    /// The account isn't listed, or no verified email matched
    NotAllowed(String),
}

impl Failure {
    /// The code the sign-in page shows (`#/login?error=…`)
    pub fn code(&self) -> &'static str {
        match self {
            Failure::Denied => "denied",
            Failure::State => "state",
            Failure::Exchange(_) => "exchange",
            Failure::NotAllowed(_) => "not_allowed",
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Trade the authorization `code` for an access token
pub async fn exchange(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    config: &ProviderConfig,
    flow: &Flow,
    code: &str,
) -> Result<String, Failure> {
    let response = http
        .post(&endpoints.token)
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &flow.redirect_uri),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("code_verifier", &flow.verifier),
        ])
        .send()
        .await
        .map_err(|e| Failure::Exchange(format!("token request failed: {e}")))?;
    let status = response.status();
    let body: TokenResponse = response
        .json()
        .await
        .map_err(|e| Failure::Exchange(format!("token response ({status}) unreadable: {e}")))?;
    // GitHub answers 200 with an `error` field
    if let Some(error) = body.error {
        let detail = body.error_description.unwrap_or_default();
        return Err(Failure::Exchange(
            format!("{error} {detail}").trim().to_string(),
        ));
    }
    match body.access_token {
        Some(token) if status.is_success() && !token.is_empty() => Ok(token),
        _ => Err(Failure::Exchange(format!(
            "token response {status} without a token"
        ))),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GoogleUser {
    email: String,
    email_verified: bool,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GithubUser {
    login: String,
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GithubEmail {
    email: String,
    verified: bool,
    primary: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DiscordUser {
    username: String,
    global_name: Option<String>,
    email: Option<String>,
    verified: bool,
}

async fn get_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<T, Failure> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| Failure::Exchange(format!("user request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Failure::Exchange(format!("user request answered {status}")));
    }
    response
        .json()
        .await
        .map_err(|e| Failure::Exchange(format!("user response unreadable: {e}")))
}

/// Who the access token belongs to, with only the emails the provider verified
pub async fn account(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    provider: Provider,
    token: &str,
) -> Result<Account, Failure> {
    match provider {
        Provider::Google => {
            let user: GoogleUser = get_json(http, &endpoints.user, token).await?;
            Ok(Account {
                name: String::new(),
                display_name: if user.name.is_empty() {
                    user.email.clone()
                } else {
                    user.name
                },
                emails: (user.email_verified && !user.email.is_empty())
                    .then_some(user.email)
                    .into_iter()
                    .collect(),
            })
        }
        Provider::Github => {
            let user: GithubUser = get_json(http, &endpoints.user, token).await?;
            let mut emails: Vec<GithubEmail> = match &endpoints.emails {
                Some(url) => get_json(http, url, token).await?,
                None => Vec::new(),
            };
            emails.sort_by_key(|e| !e.primary);
            Ok(Account {
                display_name: user
                    .name
                    .filter(|n| !n.is_empty())
                    .unwrap_or(user.login.clone()),
                name: user.login,
                emails: emails
                    .into_iter()
                    .filter(|e| e.verified && !e.email.is_empty())
                    .map(|e| e.email)
                    .collect(),
            })
        }
        Provider::Discord => {
            let user: DiscordUser = get_json(http, &endpoints.user, token).await?;
            Ok(Account {
                display_name: user
                    .global_name
                    .filter(|n| !n.is_empty())
                    .unwrap_or(user.username.clone()),
                name: user.username,
                emails: user
                    .email
                    .filter(|e| user.verified && !e.is_empty())
                    .into_iter()
                    .collect(),
            })
        }
    }
}

/// The login to keep in the session: the account name when the allow-list names it,
/// else the first verified email it allows. `None` when it allows neither.
///
/// The session keeps whichever matched, so checking it again later
/// ([`SignInConfig::allows_login`]) asks the same question.
pub fn allowed_login(
    config: &SignInConfig,
    provider: Provider,
    account: &Account,
) -> Option<String> {
    if !account.name.is_empty() && config.allows_login(provider, &account.name) {
        return Some(account.name.clone());
    }
    account
        .emails
        .iter()
        .find(|email| config.allows_login(provider, email))
        .cloned()
}

/// A `return` the UI may land on: `#/` and a short, plain route
pub fn safe_return(value: Option<&str>) -> String {
    match value {
        Some(r)
            if r.starts_with("#/")
                && r.len() <= 200
                && r[1..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "/-_.?=&".contains(c)) =>
        {
            r.to_string()
        }
        _ => "#/status".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoints_live_under_the_base() {
        let e = endpoints(Provider::Github, Some("http://127.0.0.1:9/"));
        assert_eq!(e.token, "http://127.0.0.1:9/github/token");
        assert_eq!(
            e.emails.as_deref(),
            Some("http://127.0.0.1:9/github/emails")
        );
        assert_eq!(endpoints(Provider::Google, Some("http://x")).emails, None);
        assert!(endpoints(Provider::Discord, None)
            .user
            .starts_with("https://discord.com/"));
    }

    #[test]
    fn the_authorize_url_carries_pkce_and_state() {
        let config = ProviderConfig {
            enabled: true,
            client_id: "id 1".into(),
            ..ProviderConfig::default()
        };
        let url = authorize_url(
            &endpoints(Provider::Discord, None),
            Provider::Discord,
            &config,
            "https://sync.example.com/auth/oauth/discord/callback",
            "st",
            "verifier",
        );
        let parsed = reqwest::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "id 1");
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], challenge("verifier"));
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "identify email");
    }

    #[test]
    fn the_challenge_is_rfc7636() {
        // RFC 7636 appendix B
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_matched_login_is_kept() {
        let mut config = SignInConfig::default();
        config.github.enabled = true;
        config.github.allowed_users = vec!["@example.com".into()];
        let account = Account {
            name: "octocat".into(),
            emails: vec!["octo@other.org".into(), "octo@example.com".into()],
            display_name: "Octo".into(),
        };
        assert_eq!(
            allowed_login(&config, Provider::Github, &account).as_deref(),
            Some("octo@example.com")
        );
        config.github.allowed_users.push("OctoCat".into());
        assert_eq!(
            allowed_login(&config, Provider::Github, &account).as_deref(),
            Some("octocat")
        );
        config.github.allowed_users = vec!["someone".into()];
        assert_eq!(allowed_login(&config, Provider::Github, &account), None);
    }

    #[test]
    fn returns_stay_in_the_ui() {
        assert_eq!(
            safe_return(Some("#/settings/sign-in")),
            "#/settings/sign-in"
        );
        assert_eq!(safe_return(Some("https://evil.example")), "#/status");
        assert_eq!(safe_return(Some("#/x<script>")), "#/status");
        assert_eq!(safe_return(None), "#/status");
    }
}
