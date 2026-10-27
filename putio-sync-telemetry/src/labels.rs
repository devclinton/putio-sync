//! Closed label sets. Every label value in the metric output comes from one of these
//! enums, so cardinality stays fixed and no file name, id, path or URL can leak into a
//! label (constitution Principle V, FR-003).

use std::fmt;
use std::str::FromStr;

macro_rules! label_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                #[doc = concat!("`", $value, "`")]
                $variant,
            )+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The label value.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $value),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

label_enum!(
    /// Put.io API operation, mapped from the request path.
    ApiOp {
        ListFiles => "list_files",
        GetFile => "get_file",
        DeleteFiles => "delete_files",
        Upload => "upload",
        AddTransfer => "add_transfer",
        ListTransfers => "list_transfers",
        GetTransfer => "get_transfer",
        CancelTransfers => "cancel_transfers",
        RetryTransfer => "retry_transfer",
        AccountInfo => "account_info",
        Other => "other",
    }
);

label_enum!(
    /// Outcome class of one Put.io HTTP attempt.
    ApiOutcome {
        Success => "success",
        ClientError => "client_error",
        ServerError => "server_error",
        NetworkError => "network_error",
    }
);

label_enum!(
    /// Why the retry policy scheduled another attempt.
    RetryReason {
        Network => "network",
        ServerError => "server_error",
        RateLimited => "rate_limited",
    }
);

label_enum!(
    /// Result of one sync run.
    SyncResult {
        Success => "success",
        Failure => "failure",
    }
);

label_enum!(
    /// What started an empty-folder cleanup run.
    CleanupTrigger {
        Schedule => "schedule",
        Manual => "manual",
    }
);

label_enum!(
    /// File counts reported for the latest sync run.
    SyncKind {
        Found => "found",
        Queued => "queued",
        Skipped => "skipped",
        Blocked => "blocked",
        Failed => "failed",
    }
);

label_enum!(
    /// What a folder rule did with a file.
    RuleDecision {
        Blocked => "blocked",
        Skipped => "skipped",
        Deleted => "deleted",
    }
);

label_enum!(
    /// Download queue row state.
    QueueState {
        Pending => "pending",
        Downloading => "downloading",
        Paused => "paused",
        Failed => "failed",
    }
);

label_enum!(
    /// Why a download failed.
    FailReason {
        Network => "network",
        Http => "http",
        Io => "io",
        Integrity => "integrity",
        Other => "other",
    }
);

label_enum!(
    /// Database area, by the module that ran the query.
    DbOp {
        Queue => "queue",
        History => "history",
        Uploads => "uploads",
        Migrate => "migrate",
        Other => "other",
    }
);

label_enum!(
    /// Transmission RPC method a Radarr/Sonarr request called (feature 011).
    ArrMethod {
        SessionGet => "session_get",
        TorrentAdd => "torrent_add",
        TorrentGet => "torrent_get",
        TorrentRemove => "torrent_remove",
        TorrentSet => "torrent_set",
        Other => "other",
    }
);

label_enum!(
    /// Outcome of one Transmission RPC request.
    ArrOutcome {
        Ok => "ok",
        Error => "error",
        AuthFailed => "auth_failed",
    }
);

label_enum!(
    /// Where a Radarr/Sonarr item is.
    ArrItemState {
        Putio => "putio",
        Local => "local",
        Done => "done",
        Failed => "failed",
    }
);

label_enum!(
    /// Outcome of one *arr blocklist call.
    BlocklistOutcome {
        Ok => "ok",
        NotFound => "not_found",
        Error => "error",
    }
);

impl ArrMethod {
    /// Map a Transmission method name; unknown names are `Other`.
    pub fn from_method(method: &str) -> ArrMethod {
        match method {
            "session-get" => ArrMethod::SessionGet,
            "torrent-add" => ArrMethod::TorrentAdd,
            "torrent-get" => ArrMethod::TorrentGet,
            "torrent-remove" => ArrMethod::TorrentRemove,
            "torrent-set" => ArrMethod::TorrentSet,
            _ => ArrMethod::Other,
        }
    }
}

label_enum!(
    /// How much the metrics output contains.
    Detail {
        Basic => "basic",
        Detailed => "detailed",
    }
);

impl FromStr for Detail {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "basic" => Ok(Detail::Basic),
            "detailed" => Ok(Detail::Detailed),
            other => Err(format!(
                "invalid metrics.detail '{}': expected one of basic, detailed",
                other
            )),
        }
    }
}

impl QueueState {
    /// Map a `download_queue.status` value. Statuses with no gauge (such as
    /// `completed`) return `None`.
    pub fn from_status(status: &str) -> Option<Self> {
        match status {
            "pending" => Some(QueueState::Pending),
            "downloading" => Some(QueueState::Downloading),
            "paused" => Some(QueueState::Paused),
            "failed" => Some(QueueState::Failed),
            _ => None,
        }
    }
}

impl ApiOp {
    /// Map a Put.io URL path (with or without the `/v2` prefix and query) to an operation.
    pub fn from_path(path: &str) -> ApiOp {
        let path = path.split('?').next().unwrap_or_default();
        let path = path.strip_prefix("/v2").unwrap_or(path);
        let path = path.trim_end_matches('/');
        let is_id = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());

        match path {
            "/files/list" => ApiOp::ListFiles,
            "/files/delete" => ApiOp::DeleteFiles,
            "/files/upload" => ApiOp::Upload,
            "/transfers/add" => ApiOp::AddTransfer,
            "/transfers/list" => ApiOp::ListTransfers,
            "/transfers/cancel" => ApiOp::CancelTransfers,
            "/transfers/retry" => ApiOp::RetryTransfer,
            "/account/info" => ApiOp::AccountInfo,
            _ => {
                if path.strip_prefix("/files/").is_some_and(is_id) {
                    ApiOp::GetFile
                } else if path.strip_prefix("/transfers/").is_some_and(is_id) {
                    ApiOp::GetTransfer
                } else {
                    ApiOp::Other
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_known_path() {
        let cases = [
            ("/v2/files/list", ApiOp::ListFiles),
            ("/files/list?parent_id=0", ApiOp::ListFiles),
            ("/v2/files/list?parent_id=12", ApiOp::ListFiles),
            ("/v2/files/delete", ApiOp::DeleteFiles),
            ("/v2/files/upload", ApiOp::Upload),
            ("/v2/files/123", ApiOp::GetFile),
            ("/v2/transfers/add", ApiOp::AddTransfer),
            ("/v2/transfers/list", ApiOp::ListTransfers),
            ("/v2/transfers/cancel", ApiOp::CancelTransfers),
            ("/v2/transfers/retry", ApiOp::RetryTransfer),
            ("/v2/transfers/987", ApiOp::GetTransfer),
            ("/v2/account/info", ApiOp::AccountInfo),
        ];
        for (path, op) in cases {
            assert_eq!(ApiOp::from_path(path), op, "{}", path);
        }
    }

    #[test]
    fn unknown_paths_are_other() {
        for path in ["/test", "/", "", "/v2/files/abc", "/files/", "/v2/oauth2/x"] {
            assert_eq!(ApiOp::from_path(path), ApiOp::Other, "{}", path);
        }
    }

    #[test]
    fn detail_parses_exact_values_only() {
        assert_eq!("basic".parse::<Detail>(), Ok(Detail::Basic));
        assert_eq!("detailed".parse::<Detail>(), Ok(Detail::Detailed));
        assert_eq!(
            "max".parse::<Detail>().unwrap_err(),
            "invalid metrics.detail 'max': expected one of basic, detailed"
        );
        assert!("Basic".parse::<Detail>().is_err());
    }

    #[test]
    fn queue_state_from_status() {
        assert_eq!(QueueState::from_status("paused"), Some(QueueState::Paused));
        assert_eq!(QueueState::from_status("completed"), None);
    }
}
