//! Integration tests for the login command

use assert_fs::TempDir;
use putio_sync_cli::commands::execute_login;
use putio_sync_core::config::Config;

#[tokio::test]
async fn test_login_no_verify() {
    // Create temporary directory for config
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Execute login with --no-verify flag
    let result = execute_login(
        Some("test_token_123".to_string()),
        None, // username
        None, // password
        false,
        true, // no_verify
        Some(config_path.clone()),
    )
    .await;

    // Should succeed since we're not verifying
    assert!(result.is_ok(), "Login should succeed with --no-verify");

    // Verify config file was created
    assert!(config_path.exists(), "Config file should be created");

    // Load config and verify token was saved
    let config = Config::load(&config_path).unwrap();
    assert!(config.auth.is_some());
    assert_eq!(
        config.auth.unwrap().token,
        Some("test_token_123".to_string())
    );

    // Clean up
    temp_dir.close().unwrap();
}

#[tokio::test]
async fn test_login_empty_token() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Execute login with empty token
    let result = execute_login(
        Some("".to_string()),
        None,
        None,
        false,
        true,
        Some(config_path.clone()),
    )
    .await;

    // Should fail
    assert!(result.is_err(), "Login should fail with empty token");

    // Clean up
    temp_dir.close().unwrap();
}

#[tokio::test]
async fn test_login_updates_existing_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Create initial config
    let mut initial_config = Config::default();
    initial_config.download.max_concurrent = 5; // Set a custom value
    initial_config.save(&config_path).unwrap();

    // Login with new token (no verify)
    let result = execute_login(
        Some("new_token_456".to_string()),
        None,
        None,
        false,
        true,
        Some(config_path.clone()),
    )
    .await;

    assert!(result.is_ok());

    // Load config and verify token was updated but other settings preserved
    let config = Config::load(&config_path).unwrap();
    assert!(config.auth.is_some());
    assert_eq!(
        config.auth.unwrap().token,
        Some("new_token_456".to_string())
    );
    assert_eq!(
        config.download.max_concurrent, 5,
        "Existing config values should be preserved"
    );

    // Clean up
    temp_dir.close().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn test_login_sets_secure_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Execute login
    let result = execute_login(
        Some("test_token_789".to_string()),
        None,
        None,
        false,
        true,
        Some(config_path.clone()),
    )
    .await;

    assert!(result.is_ok());

    // Verify file permissions are 0600
    let metadata = std::fs::metadata(&config_path).unwrap();
    let permissions = metadata.permissions();
    assert_eq!(
        permissions.mode() & 0o777,
        0o600,
        "Config file should have 0600 permissions"
    );

    // Clean up
    temp_dir.close().unwrap();
}

#[tokio::test]
async fn test_login_creates_nested_directories() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir
        .path()
        .join("nested")
        .join("directories")
        .join("config.toml");

    // Execute login
    let result = execute_login(
        Some("test_token_nested".to_string()),
        None,
        None,
        false,
        true,
        Some(config_path.clone()),
    )
    .await;

    assert!(result.is_ok());

    // Verify nested directories were created
    assert!(config_path.exists());
    assert!(config_path.parent().unwrap().exists());

    // Clean up
    temp_dir.close().unwrap();
}
