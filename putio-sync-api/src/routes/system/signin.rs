//! Sign-in settings (feature 019)
//!
//! - GET  /settings/sign-in - `[sign_in]` with the client secrets masked, whether a
//!   password is set (never its hash), and each provider's redirect URL
//! - PUT  /settings/sign-in - Save it. `password.new_password` sets a password,
//!   `password.clear` removes it, and a masked `client_secret` keeps the stored one.
//! - POST /settings/sign-in/sign-out-everywhere - A new session key: every session ends

use super::settings::{body, saved_message, update};
use crate::error::ApiError;
use crate::models::SuccessResponse;
use crate::signin;
use crate::AppState;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::config::Config;
use putio_sync_core::sign_in::{Provider, ProviderConfig, SignInConfig, PROVIDERS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The shortest password the settings accept
pub const MIN_PASSWORD_LENGTH: usize = 8;

/// The password as Settings shows it
#[derive(Debug, Serialize)]
pub struct PasswordView {
    pub username: String,
    pub set: bool,
}

/// A provider as Settings shows it
#[derive(Debug, Serialize)]
pub struct ProviderView {
    #[serde(flatten)]
    pub config: ProviderConfig,
    /// What to register with the provider
    pub redirect_url: String,
}

/// GET /settings/sign-in
#[derive(Debug, Serialize)]
pub struct SignInView {
    pub public_url: Option<String>,
    pub session_hours: u32,
    pub password: PasswordView,
    pub providers: BTreeMap<Provider, ProviderView>,
}

/// The password part of a PUT. Not `Debug`: it may carry a new password.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct PasswordUpdate {
    pub username: String,
    pub new_password: Option<String>,
    pub clear: bool,
}

/// PUT /settings/sign-in. Not `Debug`, like [`PasswordUpdate`].
#[derive(Deserialize)]
pub struct SignInUpdate {
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default = "default_hours")]
    pub session_hours: u32,
    #[serde(default)]
    pub password: PasswordUpdate,
    /// Providers left out keep their settings
    #[serde(default)]
    pub providers: BTreeMap<Provider, ProviderConfig>,
}

fn default_hours() -> u32 {
    putio_sync_core::sign_in::DEFAULT_SESSION_HOURS
}

/// The routes, merged into `/settings`
pub(crate) fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/sign-in", get(get_settings).put(update_settings))
        .route("/sign-in/sign-out-everywhere", post(sign_out_everywhere))
}

fn view(sign_in: &SignInConfig, headers: &HeaderMap) -> SignInView {
    let masked = sign_in.masked();
    SignInView {
        public_url: sign_in.public_url.clone(),
        session_hours: sign_in.session_hours,
        password: PasswordView {
            username: sign_in.password.username.clone(),
            set: sign_in.password.is_set(),
        },
        providers: PROVIDERS
            .into_iter()
            .map(|p| {
                (
                    p,
                    ProviderView {
                        config: masked.provider(p).clone(),
                        redirect_url: signin::redirect_url(sign_in, headers, p),
                    },
                )
            })
            .collect(),
    }
}

/// What a save compares to name the fields it changed: secrets as fingerprints, so a
/// new secret or password counts without showing either
fn change_view(config: &Config) -> serde_json::Value {
    let mut sign_in = config.sign_in.clone();
    let stamp = |s: &str| {
        if s.is_empty() {
            String::new()
        } else {
            signin::password_stamp(s)
        }
    };
    sign_in.password.password_hash = stamp(&sign_in.password.password_hash);
    for p in PROVIDERS {
        let provider = sign_in.provider_mut(p);
        provider.client_secret = stamp(&provider.client_secret);
    }
    let mut value = serde_json::json!({
        "public_url": sign_in.public_url,
        "session_hours": sign_in.session_hours,
        "password": sign_in.password,
    });
    for p in PROVIDERS {
        value[p.as_str()] = serde_json::to_value(sign_in.provider(p)).unwrap_or_default();
    }
    value
}

/// GET /settings/sign-in
async fn get_settings(State(state): State<AppState>, headers: HeaderMap) -> Json<SignInView> {
    Json(view(&state.config.read().await.sign_in, &headers))
}

/// PUT /settings/sign-in
async fn update_settings(
    State(state): State<AppState>,
    update_body: Result<Json<SignInUpdate>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let update_body = body(update_body)?;
    let new_hash = match update_body.password.new_password.as_deref() {
        Some(p) if !p.is_empty() => {
            if p.chars().count() < MIN_PASSWORD_LENGTH {
                return Err(ApiError::BadRequest(format!(
                    "sign_in.password: use at least {MIN_PASSWORD_LENGTH} characters"
                )));
            }
            let p = p.to_string();
            let hash = tokio::task::spawn_blocking(move || signin::hash_password(&p))
                .await
                .map_err(|e| ApiError::InternalError(e.to_string()))?
                .map_err(ApiError::InternalError)?;
            Some(hash)
        }
        _ => None,
    };
    let changed = update(&state, change_view, |config| {
        let current = config.sign_in.clone();
        let mut next = SignInConfig {
            public_url: update_body
                .public_url
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty()),
            session_hours: update_body.session_hours,
            password: current.password.clone(),
            ..current.clone()
        };
        let password = &update_body.password;
        next.password.username = password.username.trim().to_string();
        if password.clear {
            next.password.password_hash.clear();
        } else if let Some(hash) = new_hash {
            next.password.password_hash = hash;
        }
        for (provider, mut settings) in update_body.providers {
            settings.client_id = settings.client_id.trim().to_string();
            settings.allowed_users = settings
                .allowed_users
                .iter()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .collect();
            *next.provider_mut(provider) = settings;
        }
        config.sign_in = next.unmasked(&current);
        Ok(())
    })
    .await?;
    let mut notes = Vec::new();
    if changed.iter().any(|f| f == "password") {
        notes.push("Password sessions signed in before this end.".to_string());
    }
    Ok(Json(SuccessResponse {
        message: saved_message(
            "Sign-in",
            &changed,
            &[
                "public_url",
                "session_hours",
                "password",
                "google",
                "github",
                "discord",
            ],
            "Applies now",
            &notes,
        ),
    }))
}

/// POST /settings/sign-in/sign-out-everywhere
async fn sign_out_everywhere(State(state): State<AppState>) -> StatusCode {
    state.sign_in.new_key();
    tracing::info!("Signed out everywhere: new session key");
    StatusCode::NO_CONTENT
}
