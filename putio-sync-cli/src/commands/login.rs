//! Login command implementation
//!
//! Handles authentication with Put.io API and saves credentials to config file.

use anyhow::{anyhow, Context, Result};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::{AuthConfig, AuthMethod, Config};
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Execute the login command
///
/// # Arguments
///
/// * `token` - Optional OAuth token provided via CLI flag
/// * `username` - Optional username for app-specific password auth
/// * `password` - Optional app-specific password
/// * `interactive` - Whether to prompt for credentials interactively
/// * `no_verify` - Whether to skip credential verification
/// * `config_path` - Optional custom config file path
///
/// # Examples
///
/// ```no_run
/// # use putio_sync_cli::commands::execute_login;
/// # async fn example() {
/// // Login with token from CLI
/// execute_login(Some("my_token".to_string()), None, None, false, false, None).await.unwrap();
///
/// // Login with username and password
/// execute_login(None, Some("user@example.com".to_string()), Some("app_pass".to_string()), false, false, None).await.unwrap();
///
/// // Login interactively
/// execute_login(None, None, None, true, false, None).await.unwrap();
/// # }
/// ```
pub async fn execute_login(
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
    interactive: bool,
    no_verify: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    // Determine auth method and get credentials
    let auth_config = if interactive {
        get_credentials_interactive()?
    } else if let Some(token) = token {
        // OAuth token authentication
        if token.trim().is_empty() {
            return Err(anyhow!("Token cannot be empty"));
        }
        warn!("Warning: Providing credentials via command line may expose them in shell history");
        warn!("Consider using --interactive mode for better security");
        AuthConfig::with_token(token)
    } else if let (Some(username), Some(password)) = (username, password) {
        // App-specific password authentication
        if username.trim().is_empty() {
            return Err(anyhow!("Username cannot be empty"));
        }
        if password.trim().is_empty() {
            return Err(anyhow!("Password cannot be empty"));
        }
        warn!("Warning: Providing credentials via command line may expose them in shell history");
        warn!("Consider using --interactive mode for better security");
        AuthConfig::with_app_password(username, password)
    } else {
        // No credentials provided, prompt interactively
        get_credentials_interactive()?
    };

    // Mask credentials in logs
    let masked_creds = mask_credentials(&auth_config);
    debug!("Credentials provided: {}", masked_creds);

    // Verify credentials if requested
    if !no_verify {
        info!("Verifying credentials with Put.io API...");
        verify_credentials(&auth_config).await?;
    } else {
        warn!("Skipping credential verification (--no-verify flag set)");
    }

    // Determine config file path
    let config_file_path = determine_config_path(config_path)?;
    debug!("Config path: {}", config_file_path.display());

    // Load existing config or create a new one
    let mut config = load_or_create_config(&config_file_path)?;

    // Update config with new credentials
    config.auth = Some(auth_config.clone());

    // Save config to file (with 0600 permissions on Unix)
    info!("Saving configuration...");
    config
        .save(&config_file_path)
        .context("Failed to save configuration")?;

    info!("✓ Configuration saved to: {}", config_file_path.display());

    // Get and display account info if verification was performed
    if !no_verify {
        display_account_info_from_config(&auth_config).await?;
    }

    println!("\n✓ Successfully logged in!");
    println!("You can now use putio-sync commands.");

    Ok(())
}

/// Prompt user for credentials interactively
fn get_credentials_interactive() -> Result<AuthConfig> {
    println!("\nSelect authentication method:");
    println!("1. OAuth Token (recommended)");
    println!("2. App-Specific Password");
    print!("Enter choice (1 or 2): ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin()
        .read_line(&mut choice)
        .context("Failed to read choice from stdin")?;

    let choice = choice.trim();

    match choice {
        "1" => prompt_for_token(),
        "2" => prompt_for_credentials(),
        _ => Err(anyhow!("Invalid choice. Please enter 1 or 2.")),
    }
}

/// Prompt user for OAuth token interactively
fn prompt_for_token() -> Result<AuthConfig> {
    println!("\nPlease enter your Put.io OAuth token:");
    println!("(You can get your token from: https://app.put.io/settings/account)");
    println!("(Or use 'putio-sync get-token' command for help)");
    print!("> ");
    io::stdout().flush()?;

    let mut token = String::new();
    io::stdin()
        .read_line(&mut token)
        .context("Failed to read token from stdin")?;

    let token = token.trim().to_string();

    if token.is_empty() {
        return Err(anyhow!("Token cannot be empty"));
    }

    Ok(AuthConfig::with_token(token))
}

/// Prompt user for username and password interactively
fn prompt_for_credentials() -> Result<AuthConfig> {
    println!("\nPlease enter your Put.io username or email:");
    print!("> ");
    io::stdout().flush()?;

    let mut username = String::new();
    io::stdin()
        .read_line(&mut username)
        .context("Failed to read username from stdin")?;

    let username = username.trim().to_string();

    if username.is_empty() {
        return Err(anyhow!("Username cannot be empty"));
    }

    println!("\nPlease enter your app-specific password:");
    println!("(Generate one at: https://app.put.io/settings/account)");
    println!("(NOT your main Put.io password!)");
    print!("> ");
    io::stdout().flush()?;

    let mut password = String::new();
    io::stdin()
        .read_line(&mut password)
        .context("Failed to read password from stdin")?;

    let password = password.trim().to_string();

    if password.is_empty() {
        return Err(anyhow!("Password cannot be empty"));
    }

    Ok(AuthConfig::with_app_password(username, password))
}

/// Mask credentials for logging
fn mask_credentials(auth_config: &AuthConfig) -> String {
    match auth_config.auth_method {
        AuthMethod::Token => {
            if let Some(token) = &auth_config.token {
                if token.len() <= 8 {
                    format!("Token: {}", "*".repeat(token.len()))
                } else {
                    format!("Token: {}...", &token[..8])
                }
            } else {
                "Token: <none>".to_string()
            }
        }
        AuthMethod::AppPassword => {
            let username = auth_config.username.as_deref().unwrap_or("<none>");
            format!("Username: {}, Password: ********", username)
        }
    }
}

/// Verify credentials against Put.io API
async fn verify_credentials(auth_config: &AuthConfig) -> Result<()> {
    let client = ApiClient::from_auth_config(auth_config.clone());

    match client.verify_token().await {
        true => {
            info!("✓ Credentials verified successfully");
            Ok(())
        }
        false => Err(anyhow!(
            "Credential verification failed. Please check your credentials and try again."
        )),
    }
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
    if config_path.exists() {
        debug!("Loading existing config from: {}", config_path.display());
        Config::load(config_path).context("Failed to load existing config")
    } else {
        debug!("Creating new config (file does not exist)");
        Ok(Config::default())
    }
}

/// Display account information
async fn display_account_info_from_config(auth_config: &AuthConfig) -> Result<()> {
    let client = ApiClient::from_auth_config(auth_config.clone());

    match client.get_account_info().await {
        Ok(info) => {
            println!("\nAccount Information:");
            println!("  Username: {}", info.username);
            println!("  Email: {}", info.mail);
            println!(
                "  Disk Usage: {} / {} ({:.1}% used)",
                format_bytes(info.disk_used),
                format_bytes(info.disk_quota),
                (info.disk_used as f64 / info.disk_quota as f64) * 100.0
            );
            println!("  Available: {}", format_bytes(info.disk_available));
            Ok(())
        }
        Err(e) => {
            warn!("Could not fetch account info: {}", e);
            Ok(())
        }
    }
}

/// Format bytes into human-readable size
fn format_bytes(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size as i64, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_credentials_token() {
        let auth = AuthConfig::with_token("test_token_123".to_string());
        let masked = mask_credentials(&auth);
        assert!(masked.starts_with("Token: test_tok..."));
    }

    #[test]
    fn test_mask_credentials_short_token() {
        let auth = AuthConfig::with_token("abc".to_string());
        let masked = mask_credentials(&auth);
        assert_eq!(masked, "Token: ***");
    }

    #[test]
    fn test_mask_credentials_app_password() {
        let auth = AuthConfig::with_app_password(
            "user@example.com".to_string(),
            "password123".to_string(),
        );
        let masked = mask_credentials(&auth);
        assert_eq!(masked, "Username: user@example.com, Password: ********");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
        assert_eq!(format_bytes(5368709120), "5.00 GB");
    }
}
