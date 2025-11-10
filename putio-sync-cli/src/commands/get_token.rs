//! Get-Token command implementation
//!
//! Helper command to guide users through obtaining an OAuth token from Put.io

use anyhow::{anyhow, Context, Result};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::{AuthConfig, Config};
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{debug, info};

/// Put.io OAuth token settings URL
const PUTIO_TOKEN_URL: &str = "https://app.put.io/settings/account";

/// Execute the get-token command
///
/// # Arguments
///
/// * `save` - Whether to save the token to config file
/// * `no_browser` - Whether to skip opening the browser automatically
/// * `config_path` - Optional custom config file path
///
/// # Examples
///
/// ```no_run
/// # use putio_sync_cli::commands::execute_get_token;
/// # async fn example() {
/// // Get token and save it
/// execute_get_token(true, false, None).await.unwrap();
///
/// // Get token without opening browser
/// execute_get_token(false, true, None).await.unwrap();
/// # }
/// ```
pub async fn execute_get_token(
    save: bool,
    no_browser: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    println!("\n=== Put.io OAuth Token Helper ===\n");
    println!("This command will help you obtain an OAuth token from Put.io.");
    println!();

    // Step 1: Instructions
    println!("Step 1: Open Put.io settings");
    println!("-------------------------------");
    println!("Visit: {}", PUTIO_TOKEN_URL);
    println!();

    // Open browser if requested
    if !no_browser {
        println!("Opening browser...");
        if let Err(e) = open::that(PUTIO_TOKEN_URL) {
            eprintln!("Warning: Could not open browser automatically: {}", e);
            println!("Please manually visit the URL above.");
        } else {
            println!("Browser opened successfully!");
        }
        println!();
    }

    // Step 2: Instructions for finding token
    println!("Step 2: Find your OAuth token");
    println!("-------------------------------");
    println!("On the Put.io settings page:");
    println!("  1. Scroll down to the 'OAuth Token' section");
    println!("  2. Your token should be displayed there");
    println!("  3. Click to copy the token to your clipboard");
    println!();

    // Step 3: Prompt for token
    println!("Step 3: Enter your token");
    println!("-------------------------------");
    print!("Paste your OAuth token here: ");
    io::stdout().flush()?;

    let mut token = String::new();
    io::stdin()
        .read_line(&mut token)
        .context("Failed to read token from stdin")?;

    let token = token.trim().to_string();

    if token.is_empty() {
        return Err(anyhow!("Token cannot be empty"));
    }

    // Mask token for display
    let masked_token = mask_token(&token);
    debug!("Token received: {}", masked_token);

    println!();
    println!("Step 4: Verifying token");
    println!("-------------------------------");

    // Verify the token
    let client = ApiClient::new(token.clone());
    match client.verify_token().await {
        true => {
            println!("✓ Token verified successfully!");
        }
        false => {
            return Err(anyhow!(
                "Token verification failed. Please check your token and try again."
            ));
        }
    }

    // Display account info
    match client.get_account_info().await {
        Ok(info) => {
            println!("\nAccount Information:");
            println!("  Username: {}", info.username);
            println!("  Email: {}", info.mail);
        }
        Err(e) => {
            debug!("Could not fetch account info: {}", e);
        }
    }

    println!();

    // Save token if requested
    if save {
        println!("Step 5: Saving token");
        println!("-------------------------------");

        let config_file_path = determine_config_path(config_path)?;
        debug!("Config path: {}", config_file_path.display());

        // Load existing config or create a new one
        let mut config = load_or_create_config(&config_file_path)?;

        // Update config with new token
        config.auth = Some(AuthConfig::with_token(token));

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
        println!("To save the token, run:");
        println!("  putio-sync login --token <YOUR_TOKEN>");
        println!("Or re-run with --save flag:");
        println!("  putio-sync get-token --save");
    }

    Ok(())
}

/// Mask token for display (show only first 8 characters)
fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        "*".repeat(token.len())
    } else {
        format!("{}...", &token[..8])
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
