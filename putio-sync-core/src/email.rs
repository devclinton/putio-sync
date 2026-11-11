//! `[notifications.smtp]`: the email notification channel (feature 030). Validation,
//! defaults and password masking live here; sending is in `putio-sync-notify`.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::events::EventType;
pub use crate::hooks::MASK;

/// Events sent when `events` is not set: the same as the bundled notifiers
pub const DEFAULT_EVENTS: [&str; 7] = [
    "download.finished",
    "download.failed",
    "transfer.failed",
    "transfer.stuck",
    "sync.completed",
    "storage.low",
    "doctor.problem",
];

/// How the connection to the mail server is protected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// Plain connection upgraded with STARTTLS, which must succeed (port 587)
    #[default]
    Starttls,
    /// TLS from the first byte (port 465)
    Tls,
    /// No encryption, for a relay on the LAN; no login is allowed (port 25)
    None,
}

impl SmtpSecurity {
    /// The usual port for this kind of connection
    pub fn default_port(self) -> u16 {
        match self {
            SmtpSecurity::Starttls => 587,
            SmtpSecurity::Tls => 465,
            SmtpSecurity::None => 25,
        }
    }
}

fn default_events() -> Vec<String> {
    DEFAULT_EVENTS.iter().map(|s| s.to_string()).collect()
}

fn default_group_minutes() -> u32 {
    5
}

/// The email channel's settings
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmtpConfig {
    /// Send email
    pub enabled: bool,
    /// Mail server host name
    pub host: String,
    /// Mail server port; the usual one for `security` when unset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `starttls`, `tls` or `none`
    pub security: SmtpSecurity,
    /// Login name; empty sends no login
    pub username: String,
    /// Login password, shown as [`MASK`]
    pub password: String,
    /// Sender, `name@example.com` or `Name <name@example.com>`
    pub from: String,
    /// Recipients
    pub to: Vec<String>,
    /// Event types to send
    pub events: Vec<String>,
    /// Later events of one type within this time are sent as one summary; 0 sends each
    pub group_minutes: u32,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        SmtpConfig {
            enabled: false,
            host: String::new(),
            port: None,
            security: SmtpSecurity::default(),
            username: String::new(),
            password: String::new(),
            from: String::new(),
            to: Vec::new(),
            events: default_events(),
            group_minutes: default_group_minutes(),
        }
    }
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("enabled", &self.enabled)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("username", &self.username)
            .field(
                "password",
                &if self.password.is_empty() {
                    "<unset>"
                } else {
                    "<set>"
                },
            )
            .field("from", &self.from)
            .field("to", &self.to)
            .field("events", &self.events)
            .field("group_minutes", &self.group_minutes)
            .finish()
    }
}

/// `text` looks like `local@domain` or `Name <local@domain>`. The sender checks it fully.
pub fn is_address(text: &str) -> bool {
    let text = text.trim();
    let addr = match (text.rfind('<'), text.strip_suffix('>')) {
        (Some(open), Some(inner)) => &inner[open + 1..],
        (None, None) => text,
        _ => return false,
    };
    match addr.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.contains('@')
                && (domain.contains('.') || domain == "localhost")
                && !addr.chars().any(|c| c.is_whitespace() || c == ',')
        }
        None => false,
    }
}

impl SmtpConfig {
    /// Whether nothing was set, so the section can be left out of the file
    pub fn is_default(&self) -> bool {
        *self == SmtpConfig::default()
    }

    /// The port to connect to
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(self.security.default_port())
    }

    /// The configured event types; unknown names are skipped
    pub fn event_types(&self) -> Vec<EventType> {
        self.events.iter().filter_map(|e| e.parse().ok()).collect()
    }

    /// Check the settings. Only an enabled channel needs a server and addresses; event
    /// names and the login rule are always checked. Errors name the field.
    pub fn validate(&self) -> CoreResult<()> {
        let bad = |field: &str, why: &str| {
            Err(CoreError::ConfigError(format!(
                "notifications.smtp.{field}: {why}"
            )))
        };
        for name in &self.events {
            if name.parse::<EventType>().is_err() {
                return bad("events", &format!("unknown event \"{name}\""));
            }
        }
        if self.security == SmtpSecurity::None && !self.username.trim().is_empty() {
            return bad(
                "username",
                "a login needs security \"starttls\" or \"tls\"; it is never sent unencrypted",
            );
        }
        if self.port == Some(0) {
            return bad("port", "must be 1 to 65535");
        }
        if !self.enabled {
            return Ok(());
        }
        let host = self.host.trim();
        if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/') {
            return bad(
                "host",
                "a mail server name such as smtp.example.com is needed",
            );
        }
        if !is_address(&self.from) {
            return bad(
                "from",
                "a sender address such as putio-sync@example.com is needed",
            );
        }
        if self.to.is_empty() {
            return bad("to", "at least one recipient is needed");
        }
        if let Some(to) = self.to.iter().find(|a| !is_address(a)) {
            return bad("to", &format!("\"{to}\" is not an email address"));
        }
        Ok(())
    }

    /// A copy with the password replaced by [`MASK`] when set
    pub fn masked(&self) -> Self {
        let mut c = self.clone();
        if !c.password.is_empty() {
            c.password = MASK.to_string();
        }
        c
    }

    /// Put the stored password back where `self` carries [`MASK`]
    pub fn unmasked(mut self, current: &SmtpConfig) -> Self {
        if self.password == MASK {
            self.password = current.password.clone();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> SmtpConfig {
        SmtpConfig {
            enabled: true,
            host: "smtp.example.com".to_string(),
            from: "putio-sync <nas@example.com>".to_string(),
            to: vec!["me@example.com".to_string()],
            ..SmtpConfig::default()
        }
    }

    #[test]
    fn defaults_read_from_an_empty_section() {
        let c: SmtpConfig = toml::from_str("").unwrap();
        assert!(c.is_default());
        assert_eq!(c.port(), 587);
        assert_eq!(c.events, DEFAULT_EVENTS.to_vec());
        assert_eq!(c.group_minutes, 5);
        let tls: SmtpConfig = toml::from_str("security = \"tls\"").unwrap();
        assert_eq!(tls.port(), 465);
        c.validate().unwrap();
        ready().validate().unwrap();
    }

    #[test]
    fn bad_settings_name_the_field() {
        let cases: Vec<(SmtpConfig, &str)> = vec![
            (
                SmtpConfig {
                    host: String::new(),
                    ..ready()
                },
                "smtp.host",
            ),
            (
                SmtpConfig {
                    from: "nas".to_string(),
                    ..ready()
                },
                "smtp.from",
            ),
            (
                SmtpConfig {
                    to: vec![],
                    ..ready()
                },
                "smtp.to",
            ),
            (
                SmtpConfig {
                    to: vec!["a@example.com, b@example.com".to_string()],
                    ..ready()
                },
                "smtp.to",
            ),
            (
                SmtpConfig {
                    events: vec!["download.done".to_string()],
                    ..SmtpConfig::default()
                },
                "smtp.events",
            ),
            (
                SmtpConfig {
                    security: SmtpSecurity::None,
                    username: "me".to_string(),
                    ..ready()
                },
                "smtp.username",
            ),
        ];
        for (config, field) in cases {
            let e = config.validate().unwrap_err().to_string();
            assert!(e.contains(field), "{field}: {e}");
        }
    }

    #[test]
    fn addresses() {
        for ok in [
            "a@b.co",
            "Name <a@b.co>",
            "\"N, M\" <a@b.co>",
            "me@localhost",
        ] {
            assert!(is_address(ok), "{ok}");
        }
        for bad in [
            "", "a", "a@", "@b.co", "a@b", "a b@c.de", "<a@b.co", "a@b.co>",
        ] {
            assert!(!is_address(bad), "{bad}");
        }
    }

    #[test]
    fn the_password_is_masked_and_kept() {
        let stored = SmtpConfig {
            password: "hunter2-smtp".to_string(),
            ..ready()
        };
        let shown = stored.masked();
        assert_eq!(shown.password, MASK);
        assert!(!format!("{stored:?}").contains("hunter2"));
        assert_eq!(shown.clone().unmasked(&stored), stored);
        let changed = SmtpConfig {
            password: "new".to_string(),
            ..shown
        };
        assert_eq!(changed.unmasked(&stored).password, "new");
        assert_eq!(SmtpConfig::default().masked().password, "");
    }
}
