//! Login command implementation
//!
//! Handles authentication with Put.io API and saves credentials to config file.

use anyhow::{anyhow, Context, Result};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::{AuthConfig, Config};
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Execute the login command
///
/// # Arguments
///
/// * `token` - Optional OAuth token provided via CLI flag
/// * `interactive` - Whether to prompt for token interactively
/// * `no_verify` - Whether to skip token verification
/// * `config_path` - Optional custom config file path
///
/// # Examples
///
/// ```no_run
/// # use putio_sync_cli::commands::execute_login;
/// # async fn example() {
/// // Login with token from CLI
/// execute_login(Some("my_token".to_string()), false, false, None).await.unwrap();
///
/// // Login interactively
/// execute_login(None, true, false, None).await.unwrap();
/// # }
/// ```
pub async fn execute_login(
    token: Option<String>,
    interactive: bool,
    no_verify: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    // Get the token from CLI or interactive prompt
    let token = get_token(token, interactive)?;

    // Warn about security if token was provided via CLI
    if !interactive && !token.is_empty() {
        warn!("Warning: Providing token via command line may expose it in shell history");
        warn!("Consider using --interactive mode for better security");
    }

    // Mask token in logs (only show first 8 characters)
    let masked_token = mask_token(&token);
    debug!("Token provided: {}", masked_token);

    // Verify token if requested
    if !no_verify {
        info!("Verifying token with Put.io API...");
        verify_token(&token).await?;
    } else {
        warn!("Skipping token verification (--no-verify flag set)");
    }

    // Determine config file path
    let config_file_path = determine_config_path(config_path)?;
    debug!("Config path: {}", config_file_path.display());

    // Load existing config or create a new one
    let mut config = load_or_create_config(&config_file_path)?;

    // Update config with new token
    config.auth = Some(AuthConfig {
        token: token.clone(),
    });

    // Save config to file (with 0600 permissions on Unix)
    info!("Saving configuration...");
    config
        .save(&config_file_path)
        .context("Failed to save configuration")?;

    info!("✓ Configuration saved to: {}", config_file_path.display());

    // Get and display account info if verification was performed
    if !no_verify {
        display_account_info(&token).await?;
    }

    println!("\n✓ Successfully logged in!");
    println!("You can now use putio-sync commands.");

    Ok(())
}

/// Get token from CLI argument or interactive prompt
fn get_token(token: Option<String>, interactive: bool) -> Result<String> {
    if let Some(t) = token {
        if t.trim().is_empty() {
            return Err(anyhow!("Token cannot be empty"));
        }
        Ok(t)
    } else if interactive {
        prompt_for_token()
    } else {
        // No token provided and not interactive, prompt anyway
        prompt_for_token()
    }
}

/// Prompt user for token interactively
fn prompt_for_token() -> Result<String> {
    println!("Please enter your Put.io OAuth token:");
    println!("(You can get your token from: https://app.put.io/settings/account)");
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

    Ok(token)
}

/// Mask token for logging (show only first 8 characters)
fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        "*".repeat(token.len())
    } else {
        format!("{}...", &token[..8])
    }
}

/// Verify token against Put.io API
async fn verify_token(token: &str) -> Result<()> {
    let client = ApiClient::new(token.to_string());

    match client.verify_token().await {
        true => {
            info!("✓ Token verified successfully");
            Ok(())
        }
        false => Err(anyhow!(
            "Token verification failed. Please check your token and try again."
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
async fn display_account_info(token: &str) -> Result<()> {
    let client = ApiClient::new(token.to_string());

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
    fn test_mask_token() {
        assert_eq!(mask_token(""), "");
        assert_eq!(mask_token("abc"), "***");
        assert_eq!(mask_token("12345678"), "********");
        assert_eq!(mask_token("1234567890abcdef"), "12345678...");
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

    #[test]
    fn test_get_token_with_value() {
        let token = get_token(Some("test_token".to_string()), false);
        assert!(token.is_ok());
        assert_eq!(token.unwrap(), "test_token");
    }

    #[test]
    fn test_get_token_empty() {
        let token = get_token(Some("".to_string()), false);
        assert!(token.is_err());
        assert!(token.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_get_token_whitespace() {
        let token = get_token(Some("   ".to_string()), false);
        assert!(token.is_err());
    }
}
