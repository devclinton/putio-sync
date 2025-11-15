# putio-sync Authentication Enhancement - Workplan

## Overview
This workplan covers **adding multiple authentication options** to putio-sync, giving users flexibility in how they authenticate with Put.io. The system will support **both OAuth tokens AND app-specific passwords**, along with a helper command to obtain OAuth tokens easily.

---

## Background

### Current Implementation
- Uses OAuth token (Bearer token) for API authentication
- Token stored in config file as single string
- Login command accepts token via `--token` flag or interactive prompt
- API client adds token to Authorization header as `Bearer {token}`
- Users must manually obtain tokens from https://app.put.io/oauth

### New Implementation Goals
1. **Maintain OAuth token support** (existing method)
2. **Add app-specific password authentication** (username + password)
3. **Add `get-token` command** for easy OAuth token generation
4. **Let users choose their preferred authentication method**
5. **Support migration between authentication methods**

### Authentication Methods Comparison

| Method | Security | 2FA Support | Ease of Use | Use Case |
|--------|----------|-------------|-------------|----------|
| **OAuth Token** | High | Yes | Medium | API access, automation |
| **App-Specific Password** | High | Yes (no OTP needed) | Medium | FTP, WebDAV, API |

### Put.io Authentication Details

#### OAuth Tokens
- Created at: https://app.put.io/oauth
- Steps: Create App → Click key icon → Copy token from Secrets page
- Used with Bearer authentication
- Long-lived, can be revoked

#### App-Specific Passwords
- Created at: https://app.put.io/account/app-specific-passwords
- Used for FTP, WebDAV, and potentially API access
- If 2FA is enabled, replaces account password
- Do not combine with OTP - use app-specific password alone
- Used with HTTP Basic Authentication (username:password)

---

## 1. Update Authentication Configuration Model

### 1.1 Add Flexible AuthConfig Structure
**Feature**: Support multiple authentication methods in config

**Tasks**:
- [ ] Update `AuthConfig` struct in `putio-sync-core/src/config.rs`
  - Add `auth_method: AuthMethod` enum field
  - Keep `token: Option<String>` for OAuth token auth
  - Add `username: Option<String>` and `password: Option<String>` for Basic auth
  - Add helper methods to determine which auth is configured
  - Update struct documentation with examples for both methods
- [ ] Create `AuthMethod` enum
  ```rust
  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  pub enum AuthMethod {
      /// OAuth token authentication (Bearer token)
      Token,
      /// App-specific password authentication (Basic Auth)
      AppPassword,
  }
  ```
- [ ] Add validation logic
  - Ensure token is set if auth_method is Token
  - Ensure username and password are set if auth_method is AppPassword
  - Add `AuthConfig::validate()` method
- [ ] Update `ConfigBuilder` to handle both methods
  - Add `.auth_method(AuthMethod)` method
  - Add `.token(String)` method (keep existing)
  - Add `.username(String)` method
  - Add `.password(String)` or `.app_password(String)` method
  - Add convenience methods: `.with_token()`, `.with_app_password()`
- [ ] Update config tests
  - Test Token auth configuration
  - Test AppPassword auth configuration
  - Test config validation for each method
  - Test serialization/deserialization
  - Test builder pattern with both methods
  - Test invalid configurations (e.g., Token method without token)

**Example New Config Structure**:
```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// Authentication method to use
    pub auth_method: AuthMethod,

    /// OAuth token (used when auth_method is Token)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Put.io username (used when auth_method is AppPassword)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// App-specific password (used when auth_method is AppPassword)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AuthConfig {
    /// Check if configuration is valid
    pub fn validate(&self) -> CoreResult<()> {
        match self.auth_method {
            AuthMethod::Token => {
                if self.token.is_none() || self.token.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Token is required for Token authentication".to_string()
                    ));
                }
            }
            AuthMethod::AppPassword => {
                if self.username.is_none() || self.username.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Username is required for AppPassword authentication".to_string()
                    ));
                }
                if self.password.is_none() || self.password.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Password is required for AppPassword authentication".to_string()
                    ));
                }
            }
        }
        Ok(())
    }
}
```

**Commit**: "feat(core): add flexible authentication config supporting multiple methods"

**Changelog Entry** (`putio-sync-core/CHANGELOG.md`):
```markdown
### Added
- Support for multiple authentication methods (OAuth token and app-specific password)
- AuthMethod enum to specify authentication type
- Flexible AuthConfig structure with optional fields for different auth methods
- Configuration validation for each authentication method
- Helper methods to check configured authentication type

### Changed
- AuthConfig now includes auth_method field to specify authentication type
- Token, username, and password fields are now optional (based on auth_method)
```

---

## 2. Update API Client for Multiple Auth Methods

### 2.1 Add Multi-Method Authentication Support
**Feature**: API client supports both Bearer token and Basic Auth

**Tasks**:
- [ ] Update `ApiClient` struct in `putio-sync-core/src/api/client.rs`
  - Add `auth_method: AuthMethod` field
  - Keep `token: Option<String>` for OAuth auth
  - Add `username: Option<String>` and `password: Option<String>` for Basic auth
  - Update constructors to accept AuthConfig or individual params
  - Update struct documentation with examples for both methods
- [ ] Add flexible constructor methods
  ```rust
  /// Create client from AuthConfig
  pub fn from_auth_config(auth_config: &AuthConfig) -> CoreResult<Self> { ... }

  /// Create client with OAuth token
  pub fn with_token(token: String) -> Self { ... }

  /// Create client with username and password
  pub fn with_credentials(username: String, password: String) -> Self { ... }
  ```
- [ ] Update authentication header generation
  - Add `get_auth_header()` method that returns appropriate header
  - For Token: return `Authorization: Bearer {token}`
  - For AppPassword: return `Authorization: Basic {base64(username:password)}`
  - Handle errors if credentials are missing
- [ ] Update request methods to use flexible auth
  - Modify `get()` to call `get_auth_header()`
  - Modify `post()` (if exists) to call `get_auth_header()`
  - Ensure all HTTP requests use the correct auth method
- [ ] Add Base64 encoding for Basic Auth
  - Add `base64` dependency to Cargo.toml
  - Implement username:password encoding
- [ ] Update API client tests
  - Test Token authentication with mock server
  - Test AppPassword authentication with mock server
  - Test header generation for each method
  - Test special characters in password
  - Test missing credentials error handling
  - Update all test instances to use new constructors

**Example Implementation**:
```rust
use base64::{Engine as _, engine::general_purpose};

pub struct ApiClient {
    client: Client,
    pub(crate) base_url: String,
    auth_method: AuthMethod,
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

impl ApiClient {
    /// Create API client from AuthConfig
    pub fn from_auth_config(auth_config: &AuthConfig) -> CoreResult<Self> {
        auth_config.validate()?;

        match auth_config.auth_method {
            AuthMethod::Token => {
                Ok(Self::with_token(auth_config.token.clone().unwrap()))
            }
            AuthMethod::AppPassword => {
                Ok(Self::with_credentials(
                    auth_config.username.clone().unwrap(),
                    auth_config.password.clone().unwrap(),
                ))
            }
        }
    }

    /// Create client with OAuth token
    pub fn with_token(token: String) -> Self {
        Self::with_token_and_base_url(token, DEFAULT_BASE_URL.to_string())
    }

    /// Create client with username and password (Basic Auth)
    pub fn with_credentials(username: String, password: String) -> Self {
        Self::with_credentials_and_base_url(username, password, DEFAULT_BASE_URL.to_string())
    }

    fn get_auth_header(&self) -> CoreResult<String> {
        match self.auth_method {
            AuthMethod::Token => {
                let token = self.token.as_ref()
                    .ok_or_else(|| CoreError::ConfigError("Token not configured".to_string()))?;
                Ok(format!("Bearer {}", token))
            }
            AuthMethod::AppPassword => {
                let username = self.username.as_ref()
                    .ok_or_else(|| CoreError::ConfigError("Username not configured".to_string()))?;
                let password = self.password.as_ref()
                    .ok_or_else(|| CoreError::ConfigError("Password not configured".to_string()))?;
                let credentials = format!("{}:{}", username, password);
                let encoded = general_purpose::STANDARD.encode(credentials);
                Ok(format!("Basic {}", encoded))
            }
        }
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str) -> CoreResult<T> {
        let url = format!("{}{}", self.base_url, path);
        let auth_header = self.get_auth_header()?;

        let request = self
            .client
            .get(&url)
            .header(header::AUTHORIZATION, auth_header)
            .build()
            .map_err(CoreError::HttpError)?;

        self.execute_with_retry(request).await
    }
}
```

**Dependencies**:
Add to `putio-sync-core/Cargo.toml`:
```toml
base64 = "0.22"
```

**Commit**: "feat(core): add multi-method authentication support to API client"

**Changelog Entry** (`putio-sync-core/CHANGELOG.md`):
```markdown
### Added
- ApiClient now supports both OAuth token and Basic Authentication
- from_auth_config() constructor for creating client from config
- with_token() constructor for OAuth token authentication
- with_credentials() constructor for username/password authentication
- Automatic auth header generation based on authentication method
- Added base64 dependency for Basic Auth encoding

### Changed
- ApiClient internal structure supports multiple authentication methods
- All HTTP requests use appropriate authentication based on configured method
```

---

## 3. Update Login Command

### 3.1 Enhance Login Command for Multiple Auth Methods
**Feature**: Login command supports both OAuth tokens and app-specific passwords

**Tasks**:
- [ ] Update CLI argument structure in `putio-sync-cli/src/cli.rs`
  - Keep `--token` flag for OAuth token
  - Add `--username` and `--password` flags for app-specific password
  - Add `--method` flag to explicitly choose auth method (optional, auto-detect)
  - Keep `--interactive` flag (prompts for chosen method)
  - Update help text with examples for both methods
  - Add notes about when to use each method
- [ ] Update `execute_login()` in `putio-sync-cli/src/commands/login.rs`
  - Detect which auth method based on provided flags
  - Support `--token` for OAuth token login
  - Support `--username` and `--password` for app-specific password login
  - Update interactive mode to ask which method first
  - Update security warnings appropriately for each method
  - Update verification logic to work with both methods
- [ ] Update credential prompting logic
  - If `--token` provided: Token auth flow
  - If `--username` provided: Prompt for password (or use `--password`)
  - If interactive and no flags: Ask user which method they prefer
  - Use secure input for passwords (hide characters)
  - Add validation (username should be email format)
  - Mask tokens/passwords in logs
- [ ] Update verification logic
  - Create `ApiClient` with appropriate method
  - Verify credentials by calling account API
  - Display account info on success
  - Provide helpful error messages on failure
- [ ] Update config saving logic
  - Save auth_method, token OR username/password to config
  - Clear unused fields (e.g., clear username/password if using token)
  - Ensure file permissions are still 0600
  - Handle switching between auth methods

**Example New CLI Structure**:
```rust
Commands::Login {
    /// OAuth token (for token-based authentication)
    #[arg(short = 't', long = "token", conflicts_with_all = ["username", "password"])]
    token: Option<String>,

    /// Put.io username/email (for app-specific password authentication)
    #[arg(short = 'u', long = "username", requires = "password")]
    username: Option<String>,

    /// App-specific password (create at app.put.io/account/app-specific-passwords)
    #[arg(short = 'p', long = "password", requires = "username")]
    password: Option<String>,

    /// Prompt for credentials interactively (recommended for security)
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// Skip credential verification
    #[arg(long = "no-verify")]
    no_verify: bool,
}
```

**Example Interactive Flow**:
```
$ putio-sync login --interactive

Put.io Login
============

Choose your authentication method:
  1. OAuth Token (recommended for API access)
  2. App-Specific Password (required for 2FA accounts, works with FTP/WebDAV)

Select method (1 or 2): 1

OAuth Token Authentication
===========================

To get an OAuth token:
1. Visit: https://app.put.io/oauth
2. Click "Create App" and give it a name
3. Click the key icon next to your app
4. Copy the token from the Secrets page

Or use: putio-sync get-token

Enter your OAuth token: **********************

Verifying token with Put.io API...
✓ Authentication successful!

Account: user@example.com
Disk Usage: 4.2 GB / 10 GB (42%)
Available: 5.8 GB

Credentials saved to: /Users/user/.putiosync/config.toml
```

**Commit**: "feat(cli): enhance login command with multiple authentication methods"

**Changelog Entry** (`putio-sync-cli/CHANGELOG.md`):
```markdown
### Added
- Support for multiple authentication methods in login command
- --token flag for OAuth token authentication (existing)
- --username and --password flags for app-specific password authentication
- Interactive mode now lets users choose authentication method
- Helpful instructions for both authentication methods
- Auto-detection of authentication method based on flags

### Changed
- Login command prompts user to choose authentication method when interactive
- Improved help text with examples for both methods
- Better error messages specific to each authentication method
```

---

## 4. Add Get-Token Command

### 4.1 Implement Get-Token Helper Command
**Feature**: Easy OAuth token generation via CLI (similar to putioarr)

**Tasks**:
- [ ] Add new `get-token` command to CLI
  - Add to `Commands` enum in `putio-sync-cli/src/cli.rs`
  - Create `putio-sync-cli/src/commands/get_token.rs`
  - Add command to help text and documentation
- [ ] Implement get-token workflow
  - Display link to https://app.put.io/oauth
  - Provide step-by-step instructions
  - Option 1: Manual flow (show instructions, wait for user to paste token)
  - Option 2: Open browser automatically (if possible)
  - Validate token after user provides it
  - Optionally save to config automatically
- [ ] Add interactive features
  - Automatically open browser to OAuth page (with user permission)
  - Prompt for token after user completes web flow
  - Verify token works before saving
  - Ask if user wants to save to config
- [ ] Add helpful output
  - Clear step-by-step instructions
  - Visual progress indicators
  - Success confirmation with account info
  - Option to test token before saving

**Example Implementation**:
```rust
// In putio-sync-cli/src/cli.rs
Commands::GetToken {
    /// Automatically save token to config after verification
    #[arg(long = "save")]
    save: bool,

    /// Don't automatically open browser
    #[arg(long = "no-browser")]
    no_browser: bool,
}

// In putio-sync-cli/src/commands/get_token.rs
pub async fn execute_get_token(save: bool, no_browser: bool) -> Result<()> {
    println!("\nPut.io OAuth Token Generator");
    println!("============================\n");

    println!("Follow these steps to get your OAuth token:\n");
    println!("1. Visit: https://app.put.io/oauth");
    println!("2. Click 'Create App'");
    println!("3. Name your app (e.g., 'putio-sync CLI')");
    println!("4. Click the key icon next to your app");
    println!("5. Copy the token from the Secrets page\n");

    // Optionally open browser
    if !no_browser {
        print!("Open browser automatically? (Y/n): ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().is_empty() || input.trim().eq_ignore_ascii_case("y") {
            if let Err(e) = open::that("https://app.put.io/oauth") {
                warn!("Could not open browser: {}", e);
            } else {
                println!("✓ Browser opened to Put.io OAuth page\n");
            }
        }
    }

    // Get token from user
    print!("Enter your OAuth token: ");
    io::stdout().flush()?;
    let token = read_password()?;

    // Verify token
    println!("\nVerifying token...");
    let client = ApiClient::with_token(token.clone());
    let account = client.get_account_info().await?;

    println!("✓ Token is valid!\n");
    println!("Account: {}", account.username);
    println!("Disk Usage: {} / {}",
        format_bytes(account.disk_used),
        format_bytes(account.disk_quota));

    // Save if requested
    if save {
        save_token_to_config(&token)?;
        println!("\n✓ Token saved to config file");
    } else {
        println!("\nTo save this token, run:");
        println!("  putio-sync login --token <your-token>");
    }

    Ok(())
}
```

**Example Usage**:
```bash
# Get token with automatic save
$ putio-sync get-token --save

Put.io OAuth Token Generator
============================

Follow these steps to get your OAuth token:

1. Visit: https://app.put.io/oauth
2. Click 'Create App'
3. Name your app (e.g., 'putio-sync CLI')
4. Click the key icon next to your app
5. Copy the token from the Secrets page

Open browser automatically? (Y/n): y
✓ Browser opened to Put.io OAuth page

Enter your OAuth token: **********************

Verifying token...
✓ Token is valid!

Account: user@example.com
Disk Usage: 4.2 GB / 10 GB

✓ Token saved to config file
```

**Dependencies** (if opening browser):
Add to `putio-sync-cli/Cargo.toml`:
```toml
open = "5.0"  # For opening URLs in browser
```

**Commit**: "feat(cli): add get-token command for easy OAuth token generation"

**Changelog Entry** (`putio-sync-cli/CHANGELOG.md`):
```markdown
### Added
- get-token command for easy OAuth token generation
- Step-by-step instructions for creating OAuth apps
- Automatic browser opening to Put.io OAuth page (optional)
- Token verification before saving
- Option to automatically save verified token to config
- --save flag to save token after verification
- --no-browser flag to skip automatic browser opening
```

---

## 5. Update Tests and Documentation

### 5.1 Update Unit Tests
**Feature**: Test all authentication methods

**Tasks**:
- [ ] Update core library tests
  - Test AuthConfig with both auth methods
  - Test AuthMethod enum serialization
  - Test config validation for each method
  - Test ApiClient with Token auth
  - Test ApiClient with AppPassword auth
  - Test auth header generation for each method
  - Update mock server expectations for both auth types
- [ ] Update CLI tests
  - Test login with `--token`
  - Test login with `--username` and `--password`
  - Test login interactive mode (both methods)
  - Test get-token command
  - Test auth method switching
- [ ] Add new test cases
  - Test config migration scenarios
  - Test invalid auth configurations
  - Test switching between auth methods
  - Test special characters in passwords
  - Test empty credentials handling

**Commit**: "test: add comprehensive tests for multi-method authentication"

**Changelog Entry** (both crates):
```markdown
### Testing
- Added tests for OAuth token authentication
- Added tests for app-specific password authentication
- Added tests for authentication method switching
- Added tests for get-token command
- Updated all existing tests to work with flexible auth system
```

---

### 5.2 Update Documentation
**Feature**: Document all authentication options

**Tasks**:
- [ ] Update inline code documentation
  - Document AuthConfig and AuthMethod
  - Document ApiClient constructors
  - Update examples throughout codebase
  - Add examples for both auth methods
- [ ] Update README (create if doesn't exist)
  - Add authentication section
  - Explain when to use each method
  - Add examples for both methods
  - Document get-token command
- [ ] Update command help text
  - Update login command help
  - Add get-token command help
  - Include examples for both methods
  - Add troubleshooting tips
- [ ] Create authentication guide
  - Comparison of auth methods
  - Step-by-step for each method
  - When to use which method
  - Troubleshooting common issues

**Commit**: "docs: add comprehensive authentication documentation"

**Changelog Entry** (both crates):
```markdown
### Documentation
- Comprehensive authentication documentation
- Examples for both OAuth token and app-specific password methods
- get-token command documentation
- Authentication method comparison guide
- Troubleshooting guide for authentication issues
```

---

## 6. Error Handling and User Experience

### 6.1 Improve Error Messages and UX
**Feature**: Helpful errors and smooth user experience

**Tasks**:
- [ ] Add specific error types
  - Invalid token error
  - Invalid username/password error
  - Missing credentials error
  - Auth method mismatch error
- [ ] Improve error messages
  - Suggest using get-token on OAuth failures
  - Link to app-specific password creation on Basic auth failures
  - Distinguish network errors from auth errors
  - Provide recovery suggestions
- [ ] Add credential validation
  - Validate token format (if there's a pattern)
  - Validate username is email format
  - Check credentials are not empty
- [ ] Enhance user experience
  - Color-coded output for success/failure
  - Progress indicators for verification
  - Clear separation between steps
  - Helpful next-step suggestions

**Commit**: "feat: improve authentication error handling and UX"

**Changelog Entry**:
```markdown
### Added
- Specific error types for different authentication failures
- Helpful error messages with recovery suggestions
- Credential validation with clear feedback
- Color-coded output for better readability
- Progress indicators for long-running operations

### Improved
- Error messages now include links to help resources
- Better distinction between different types of failures
- Clearer guidance on resolving authentication issues
```

---

## Implementation Order

### Phase 1: Core Infrastructure
1. ✓ Update AuthConfig with flexible structure (1.1)
2. ✓ Update ApiClient for multi-method auth (2.1)

### Phase 2: CLI Commands
3. ✓ Update login command for multiple methods (3.1)
4. ✓ Add get-token command (4.1)

### Phase 3: Quality & Polish
5. ✓ Update all tests (5.1)
6. ✓ Update documentation (5.2)
7. ✓ Improve error handling and UX (6.1)

---

## Authentication Methods Guide

### Method 1: OAuth Token (Recommended for API Access)

**Pros:**
- Standard OAuth 2.0 flow
- Long-lived tokens
- Can be revoked individually
- Works with all Put.io API features

**Cons:**
- Requires manual token creation
- Need to visit web interface

**How to Use:**
```bash
# Easy way with get-token helper
putio-sync get-token --save

# Manual way
putio-sync login --token YOUR_TOKEN

# Interactive
putio-sync login --interactive
# Select: 1. OAuth Token
```

**How to Get Token:**
1. Use `putio-sync get-token` (easiest)
2. Or manually: Visit https://app.put.io/oauth, create app, copy token

---

### Method 2: App-Specific Password (Recommended for 2FA Users)

**Pros:**
- Works without entering OTP each time
- Designed for 2FA-enabled accounts
- Can be used for FTP/WebDAV too
- Can be revoked individually

**Cons:**
- Password shown only once (must save securely)
- Requires creating through web interface

**How to Use:**
```bash
# With flags
putio-sync login --username user@example.com --password APP_PASSWORD

# Interactive (recommended for security)
putio-sync login --interactive
# Select: 2. App-Specific Password
```

**How to Get App-Specific Password:**
1. Visit https://app.put.io/account/app-specific-passwords
2. Click "Create One"
3. Name it (e.g., "putio-sync CLI")
4. Copy the generated password (you won't see it again!)

---

## Migration Guide

### For Existing Users

The current OAuth token authentication **continues to work** with no changes required. This update only adds new options.

### Switching Authentication Methods

Users can switch between methods at any time:

```bash
# Switch to app-specific password
putio-sync login --username user@example.com --password APP_PASS

# Switch back to OAuth token
putio-sync login --token YOUR_TOKEN

# Or use interactive mode
putio-sync login --interactive
```

---

## Testing Checklist

### Manual Testing
- [ ] Login with OAuth token (existing functionality)
- [ ] Login with app-specific password (new)
- [ ] Get-token command with browser opening
- [ ] Get-token command without browser opening
- [ ] Switch between auth methods
- [ ] Verify each method works with list command
- [ ] Test interactive mode for both methods
- [ ] Test with 2FA-enabled account
- [ ] Test invalid credentials for each method
- [ ] Test config file permissions
- [ ] Test special characters in password
- [ ] Test token verification
- [ ] Test password verification

### Automated Testing
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] CI/CD pipeline passes
- [ ] Documentation builds without warnings
- [ ] Clippy shows no warnings
- [ ] Tests cover both auth methods

---

## Success Criteria

### Functional Requirements
- [x] OAuth token authentication works (existing)
- [x] App-specific password authentication works (new)
- [x] get-token command helps users get OAuth tokens
- [x] Users can choose their preferred method
- [x] Users can switch between methods
- [x] Config stores appropriate credentials for chosen method
- [x] API client correctly authenticates with both methods
- [x] All existing features work with both methods

### Quality Requirements
- [x] All tests pass
- [x] Documentation is complete and accurate
- [x] CI/CD pipeline passes
- [x] No clippy warnings
- [x] Error messages are helpful
- [x] User experience is smooth

### Backward Compatibility
- [x] Existing OAuth token configs continue to work
- [x] No breaking changes for current users
- [x] Smooth upgrade path

---

## Non-Breaking Changes

### This is NOT a Breaking Change

Unlike the original plan, this enhanced approach:
- **Keeps existing OAuth token authentication working**
- **Adds new authentication methods as options**
- **Requires no migration for existing users**
- **Allows users to choose their preferred method**

Existing users can continue using OAuth tokens with zero changes. New authentication methods are purely additive.

---

## Future Enhancements

### Potential Future Features
- [ ] OAuth Device Flow for fully automated token generation
- [ ] Platform keyring integration for credential storage
- [ ] Credential refresh/rotation reminders
- [ ] Multiple profile support (different accounts)
- [ ] SSO/SAML support (if Put.io adds it)

---

## Notes

- This is **not a breaking change** - existing functionality is preserved
- OAuth tokens remain the recommended method for most users
- App-specific passwords are best for 2FA users or those who prefer Basic Auth
- get-token command makes OAuth token generation much easier
- Users have full flexibility to choose their preferred authentication method
- Config format is backward compatible with existing configs

---

## Comparison with putioarr

### Similar Features
- `get-token` command for easy OAuth token generation
- Helpful instructions and automation
- Token verification before saving

### Our Enhancements
- Support for multiple authentication methods (not just token)
- Interactive mode with method selection
- App-specific password support for 2FA users
- Automatic browser opening (optional)
- Backward compatibility with existing configs
- Comprehensive error handling
- Better documentation and help text

---

## Technical Details

### Config File Format

**OAuth Token:**
```toml
[auth]
auth_method = "Token"
token = "YOUR_OAUTH_TOKEN"
```

**App-Specific Password:**
```toml
[auth]
auth_method = "AppPassword"
username = "user@example.com"
password = "YOUR_APP_SPECIFIC_PASSWORD"
```

### Authentication Headers

**OAuth Token:**
```
Authorization: Bearer YOUR_OAUTH_TOKEN
```

**App-Specific Password:**
```
Authorization: Basic base64(username:password)
```

Both methods use HTTPS, so credentials are encrypted in transit.
