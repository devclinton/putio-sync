//! Authentication configuration for Put.io API client

use crate::error::{ClientError, ClientResult};
use serde::{Deserialize, Serialize};

/// Authentication method
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AuthMethod {
    /// OAuth token authentication (Bearer token)
    #[default]
    Token,
    /// App-specific password authentication (Basic Auth)
    AppPassword,
}

/// Authentication configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// Authentication method to use
    #[serde(default)]
    pub auth_method: AuthMethod,

    /// OAuth token (used when auth_method is Token)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Put.io username/email (used when auth_method is AppPassword)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// App-specific password (used when auth_method is AppPassword)
    /// Create at: https://app.put.io/account/app-specific-passwords
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AuthConfig {
    /// Create a new AuthConfig with OAuth token
    pub fn with_token(token: String) -> Self {
        Self {
            auth_method: AuthMethod::Token,
            token: Some(token),
            username: None,
            password: None,
        }
    }

    /// Create a new AuthConfig with app-specific password
    pub fn with_app_password(username: String, password: String) -> Self {
        Self {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some(username),
            password: Some(password),
        }
    }

    /// Validate the authentication configuration
    pub fn validate(&self) -> ClientResult<()> {
        match self.auth_method {
            AuthMethod::Token => {
                if self.token.is_none() || self.token.as_ref().unwrap().is_empty() {
                    return Err(ClientError::AuthError(
                        "Token is required for Token authentication".to_string(),
                    ));
                }
            }
            AuthMethod::AppPassword => {
                if self.username.is_none() || self.username.as_ref().unwrap().is_empty() {
                    return Err(ClientError::AuthError(
                        "Username is required for AppPassword authentication".to_string(),
                    ));
                }
                if self.password.is_none() || self.password.as_ref().unwrap().is_empty() {
                    return Err(ClientError::AuthError(
                        "Password is required for AppPassword authentication".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_config_with_token() {
        let auth = AuthConfig::with_token("test_token".to_string());
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token");
        assert!(auth.username.is_none());
        assert!(auth.password.is_none());
        assert!(auth.validate().is_ok());
    }

    #[test]
    fn test_auth_config_with_app_password() {
        let auth =
            AuthConfig::with_app_password("user@example.com".to_string(), "password123".to_string());
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "password123");
        assert!(auth.token.is_none());
        assert!(auth.validate().is_ok());
    }

    #[test]
    fn test_validate_token_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::Token,
            token: None,
            username: None,
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_validate_token_empty() {
        let auth = AuthConfig {
            auth_method: AuthMethod::Token,
            token: Some("".to_string()),
            username: None,
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_validate_app_password_missing_username() {
        let auth = AuthConfig {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: None,
            password: Some("password".to_string()),
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_validate_app_password_missing_password() {
        let auth = AuthConfig {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some("user".to_string()),
            password: None,
        };
        assert!(auth.validate().is_err());
    }
}
