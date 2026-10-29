//! The check on Put.io of a magnet fallback once its transfer finished (042 US3, US4),
//! shared by the *arr client and the watch folders. The caller records the result.

use std::time::Duration;

use putio_sync_client::{error_chain, PutioApiClient, Transfer};
use putio_sync_telemetry::precheck as metrics;
use putio_sync_telemetry::FallbackResult;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::{Metainfo, Outcome, Policy, PrecheckSource, Prechecker, Reason, Stage, Verdict};

/// How long listing a fallback's files on Put.io may take
pub const LIST_BUDGET: Duration = Duration::from_secs(5);

/// Folders listed at once for that
const LIST_CONCURRENCY: usize = 4;

/// What the check on Put.io decided
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnPutio {
    /// The files couldn't be listed or deleted; the next pass tries again
    Retry,
    /// Go on as before (also a rejection under `dry_run`)
    Passed,
    /// Deleted from Put.io; the file list is the one that was walked
    Rejected {
        meta: Option<Metainfo>,
        reason: Reason,
    },
}

/// Bytes Put.io downloaded for a transfer
pub fn putio_bytes(transfer: &Transfer) -> i64 {
    transfer.downloaded.unwrap_or(0).max(0)
}

/// A transfer whose files are all on Put.io
pub fn is_finished(transfer: &Transfer) -> bool {
    matches!(transfer.status.as_str(), "COMPLETED" | "SEEDING")
}

/// A transfer whose check can no longer happen: in `ERROR` or cancelled. A transfer
/// missing from the list is gone too.
pub fn is_gone(transfer: &Transfer) -> bool {
    matches!(transfer.status.as_str(), "ERROR" | "CANCELLED")
}

impl Prechecker {
    /// Walk `file_id`, judge it, and on a rejection outside `dry_run` remove it from
    /// Put.io. Logs and counts the check and the fallback.
    #[allow(clippy::too_many_arguments)]
    pub async fn judge_on_putio(
        &self,
        client: &PutioApiClient,
        hash: &str,
        name: &str,
        transfer: &Transfer,
        file_id: i64,
        policy: &Policy,
        source: PrecheckSource,
    ) -> OnPutio {
        let deadline = Instant::now() + LIST_BUDGET;
        let tree = client.walk_tree(file_id, LIST_CONCURRENCY, deadline).await;
        let Some(checked) = Prechecker::check_on_putio(hash, name, &tree, policy) else {
            warn!(
                hash,
                name,
                transfer_id = transfer.id,
                "Could not list the release's files on Put.io; checking again next pass"
            );
            return OnPutio::Retry;
        };
        let rejected = matches!(checked.verdict, Verdict::Reject(_));
        if rejected
            && !self.config().dry_run
            && !clean_up(client, hash, name, transfer.id, file_id).await
        {
            return OnPutio::Retry;
        }
        let result = if rejected {
            FallbackResult::Rejected
        } else {
            FallbackResult::Passed
        };
        metrics::fallback(result, putio_bytes(transfer) as u64);
        match self.decide(checked, source, Stage::Putio) {
            Outcome::Rejected(meta, reason) => OnPutio::Rejected { meta, reason },
            _ => OnPutio::Passed,
        }
    }
}

/// Remove a rejected fallback from Put.io (research R4). The files go first, so a failure
/// leaves the transfer for the next pass to find and try again. False when they couldn't
/// be deleted.
pub async fn clean_up(
    client: &PutioApiClient,
    hash: &str,
    name: &str,
    transfer_id: i64,
    file_id: i64,
) -> bool {
    if let Err(e) = client.delete_files(vec![file_id]).await {
        warn!(
            hash,
            name,
            transfer_id,
            error = %error_chain(&e),
            "Failed to delete a rejected release on Put.io; retrying next pass"
        );
        return false;
    }
    if let Err(e) = client.cancel_transfers(vec![transfer_id]).await {
        warn!(
            hash,
            name,
            transfer_id,
            error = %error_chain(&e),
            "Failed to cancel a rejected transfer on Put.io"
        );
    }
    if let Err(e) = client.clean_transfers(vec![transfer_id]).await {
        debug!(transfer_id, error = %error_chain(&e), "Failed to clear a rejected transfer");
    }
    true
}
