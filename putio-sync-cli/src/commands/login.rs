//! Login command implementation
//!
//! Implements OAuth 2.0 Authorization Code Flow for obtaining access tokens from Put.io

use anyhow::{anyhow, Context, Result};
use axum::{extract::Query, response::Html, routing::get, Router};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::{AuthConfig, Config};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tracing::{debug, info};

/// Put.io OAuth authorization endpoint
const PUTIO_AUTH_URL: &str = "https://api.put.io/v2/oauth2/authenticate";

/// Put.io OAuth token endpoint
const PUTIO_TOKEN_URL: &str = "https://api.put.io/v2/oauth2/access_token";

/// OAuth callback query parameters
#[derive(Debug, Deserialize)]
struct AuthCallbackParams {
    code: String,
    #[allow(dead_code)]
    state: String,
}

/// Put.io token response
#[derive(Debug, Deserialize)]
struct PutioTokenResponse {
    access_token: String,
}

/// Execute the login command using OAuth 2.0 flow
///
/// # Arguments
///
/// * `no_save` - Whether to skip saving the token to config file (saves by default)
/// * `no_browser` - Whether to skip opening the browser automatically
/// * `config_path` - Optional custom config file path
///
/// # Examples
///
/// ```no_run
/// # use putio_sync_cli::commands::execute_login;
/// # async fn example() {
/// // Login and save token (default behavior)
/// execute_login(false, false, None).await.unwrap();
///
/// // Login without opening browser
/// execute_login(false, true, None).await.unwrap();
/// # }
/// ```
pub async fn execute_login(
    no_save: bool,
    no_browser: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    println!("\n=== Put.io OAuth Token Helper ===\n");
    println!("This command will help you obtain an OAuth token from Put.io using OAuth 2.0.");
    println!();

    // Load OAuth configuration from .env file
    let oauth_config = load_oauth_config()?;

    // Create OAuth client
    let client = create_oauth_client(&oauth_config)?;

    // Generate authorization URL
    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("*".to_string()))
        .url();

    println!("Step 1: Starting local callback server");
    println!("-------------------------------");
    println!("Starting server on {}...", oauth_config.redirect_uri);

    // Parse redirect URI to get port
    let port = extract_port(&oauth_config.redirect_uri)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // Create channel to receive authorization code
    let (tx, rx) = oneshot::channel::<Result<String>>();

    // Start callback server
    let app = create_callback_app(tx, csrf_token.secret().clone());

    // Bind TCP listener
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to address")?;

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .context("Failed to start callback server")
    });

    println!("✓ Server started successfully!\n");

    // Open browser to authorization URL
    println!("Step 2: Authorize putio-sync");
    println!("-------------------------------");
    if !no_browser {
        println!("Opening browser to Put.io authorization page...");
        if let Err(e) = open::that(auth_url.as_str()) {
            eprintln!("Warning: Could not open browser automatically: {}", e);
            println!("Please manually visit this URL:");
            println!("{}", auth_url);
        } else {
            println!("✓ Browser opened successfully!");
        }
    } else {
        println!("Please visit this URL to authorize putio-sync:");
        println!("{}", auth_url);
    }
    println!();
    println!("Waiting for authorization...");

    // Wait for authorization code
    let auth_code = rx
        .await
        .context("Failed to receive authorization code")?
        .context("Authorization failed")?;

    println!("✓ Authorization received!\n");

    // Exchange authorization code for access token
    println!("Step 3: Exchanging code for access token");
    println!("-------------------------------");

    // Put.io uses a non-standard OAuth implementation with GET instead of POST
    // Build the token exchange URL with query parameters
    let token_url = format!(
        "{}?client_id={}&client_secret={}&grant_type=authorization_code&redirect_uri={}&code={}",
        PUTIO_TOKEN_URL,
        urlencoding::encode(&oauth_config.client_id),
        urlencoding::encode(&oauth_config.client_secret),
        urlencoding::encode(&oauth_config.redirect_uri),
        urlencoding::encode(&auth_code)
    );

    debug!("Exchanging authorization code for token");

    // Make GET request to exchange code for token
    let http_client = reqwest::Client::new();
    let response = http_client
        .get(&token_url)
        .send()
        .await
        .context("Failed to send token exchange request")?;

    let status = response.status();
    let response_text = response.text().await.context("Failed to read response body")?;

    debug!("Token exchange response (status {}): {}", status, response_text);

    if !status.is_success() {
        tracing::error!("Token exchange failed with status {}: {}", status, response_text);
        return Err(anyhow!("Failed to exchange authorization code for token: HTTP {} - {}", status, response_text));
    }

    let token_response: PutioTokenResponse = serde_json::from_str(&response_text)
        .context(format!("Failed to parse token response. Response was: {}", response_text))?;

    let access_token = token_response.access_token;

    println!("✓ Access token received!\n");

    // Verify token works
    println!("Step 4: Verifying token");
    println!("-------------------------------");

    let api_client = ApiClient::new(access_token.clone());

    // Try to get account info to verify token and display info
    match api_client.get_account_info().await {
        Ok(info) => {
            println!("✓ Token verified successfully!");
            println!("\nAccount Information:");
            println!("  Username: {}", info.username);
            println!("  Email: {}", info.mail);
        }
        Err(e) => {
            tracing::error!("Token verification failed: {:?}", e);
            return Err(anyhow!("Token verification failed: {}", e));
        }
    }

    println!();

    // Save token by default (unless --no-save is specified)
    if !no_save {
        println!("Step 5: Saving token");
        println!("-------------------------------");

        let config_file_path = determine_config_path(config_path)?;
        debug!("Config path: {}", config_file_path.display());

        // Load existing config or create a new one
        let mut config = load_or_create_config(&config_file_path)?;

        // Update config with new token
        config.auth = Some(AuthConfig::with_token(access_token));

        // Save config to file (with 0600 permissions on Unix)
        info!("Saving configuration...");
        config
            .save(&config_file_path)
            .context("Failed to save configuration")?;

        println!("✓ Token saved to: {}", config_file_path.display());
        println!();
        println!("✓ All done! You can now use putio-sync commands.");
    } else {
        println!("Token verified but not saved.");
        println!("Your access token is:");
        println!("{}", access_token);
        println!();
        println!("To save the token, run:");
        println!("  putio-sync login --token {}", &access_token[..20]);
        println!("Or re-run with --save flag:");
        println!("  putio-sync get-token --save");
    }

    // Shutdown server
    server_handle.abort();

    Ok(())
}

/// OAuth configuration loaded from .env
#[derive(Debug)]
struct OAuthConfig {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

/// Load OAuth configuration from .env file
fn load_oauth_config() -> Result<OAuthConfig> {
    // Load .env file from project root
    dotenvy::dotenv().context("Failed to load .env file. Make sure .env exists in the project root with PUTIO_CLIENT_ID, PUTIO_CLIENT_SECRET, and PUTIO_REDIRECT_URI")?;

    let client_id = std::env::var("PUTIO_CLIENT_ID")
        .context("PUTIO_CLIENT_ID not found in .env file")?;

    let client_secret = std::env::var("PUTIO_CLIENT_SECRET")
        .context("PUTIO_CLIENT_SECRET not found in .env file")?;

    let redirect_uri = std::env::var("PUTIO_REDIRECT_URI")
        .context("PUTIO_REDIRECT_URI not found in .env file")?;

    Ok(OAuthConfig {
        client_id,
        client_secret,
        redirect_uri,
    })
}

/// Create OAuth client
fn create_oauth_client(config: &OAuthConfig) -> Result<BasicClient> {
    let client = BasicClient::new(
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(config.client_secret.clone())),
        AuthUrl::new(PUTIO_AUTH_URL.to_string())
            .context("Invalid authorization URL")?,
        Some(
            TokenUrl::new(PUTIO_TOKEN_URL.to_string())
                .context("Invalid token URL")?,
        ),
    )
    .set_redirect_uri(
        RedirectUrl::new(config.redirect_uri.clone())
            .context("Invalid redirect URI")?,
    );

    Ok(client)
}

/// Extract port from redirect URI
fn extract_port(redirect_uri: &str) -> Result<u16> {
    let url = url::Url::parse(redirect_uri)
        .context("Invalid redirect URI format")?;

    url.port()
        .ok_or_else(|| anyhow!("Redirect URI must include a port (e.g., http://localhost:29876/auth/callback)"))
}

/// Create callback HTTP app
fn create_callback_app(
    tx: oneshot::Sender<Result<String>>,
    expected_state: String,
) -> Router {
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let expected_state = Arc::new(expected_state);

    Router::new().route(
        "/auth/callback",
        get(move |Query(params): Query<AuthCallbackParams>| {
            let tx = tx.clone();
            let expected_state = expected_state.clone();

            async move {
                // Verify CSRF token
                if params.state != *expected_state {
                    let _ = tx
                        .lock()
                        .await
                        .take()
                        .map(|tx| tx.send(Err(anyhow!("CSRF token mismatch"))));

                    return Html(
                        "<html><body><h1>Error</h1><p>CSRF token mismatch. Please try again.</p></body></html>"
                    );
                }

                // Send authorization code
                let _ = tx
                    .lock()
                    .await
                    .take()
                    .map(|tx| tx.send(Ok(params.code)));

                Html(
                    "<html><body><h1>Authorization Successful!</h1><p>You can close this window and return to the terminal.</p></body></html>"
                )
            }
        }),
    )
}

/// Determine the config file path to use
fn determine_config_path(config_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = config_path {
        Ok(path)
    } else {
        Config::default_path().context("Failed to determine default config path")
    }
}

/// Load existing config or create a new default config
fn load_or_create_config(config_path: &PathBuf) -> Result<Config> {
    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            debug!("Creating parent directory: {}", parent.display());
            std::fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }
    }

    if config_path.exists() {
        debug!("Loading existing config from: {}", config_path.display());
        match Config::load(config_path) {
            Ok(config) => Ok(config),
            Err(e) => {
                debug!("Failed to load existing config: {}, using default config", e);
                // If config is corrupted or empty, use default
                Ok(Config::default())
            }
        }
    } else {
        debug!("Creating new config (file does not exist)");
        Ok(Config::default())
    }
}
