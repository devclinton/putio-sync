//! How putio-sync was installed, which decides how it is updated (research R10).
//!
//! Detection comes with US2; until then every install is `Unknown` and the notice shows
//! only the release link.

use serde::Serialize;

/// How an update is applied
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    /// Download, verify, swap the binary and restart
    Swap,
    /// Download and open the platform installer
    Installer,
    /// Show the channel's own update command
    Instructions,
}

/// An install channel
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// Not recognised
    Unknown,
}

impl Channel {
    /// Work out the channel of the running binary
    pub fn detect() -> Self {
        Self::Unknown
    }

    /// The `kind` the API and `GET /status` report
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
        }
    }

    /// A name for people
    pub fn label(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown install",
        }
    }

    /// How this channel is updated
    pub fn mode(&self) -> UpdateMode {
        match self {
            Self::Unknown => UpdateMode::Instructions,
        }
    }
}
