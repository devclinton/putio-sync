//! Gauges refreshed on a timer: queue depth by state and disk space.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::{QueueState, Telemetry};

/// How often the gauges refresh (R7).
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(15);

impl Telemetry {
    /// Set `queue_items{state}`. States missing from `counts` are set to 0.
    pub fn set_queue_counts(&self, counts: &[(QueueState, i64)]) {
        for state in QueueState::ALL {
            let n = counts
                .iter()
                .find(|(s, _)| s == state)
                .map_or(0, |(_, n)| *n);
            self.queue_items.with_label_values(&[state.as_str()]).set(n);
        }
    }

    /// Set the disk gauges from `statvfs(dir)`. On error both gauges are removed.
    pub fn refresh_disk(&self, dir: &Path) -> std::io::Result<()> {
        match rustix::fs::statvfs(dir) {
            Ok(st) => {
                let frsize = st.f_frsize as f64;
                self.disk_free
                    .with_label_values::<&str>(&[])
                    .set(st.f_bavail as f64 * frsize);
                self.disk_total
                    .with_label_values::<&str>(&[])
                    .set(st.f_blocks as f64 * frsize);
                Ok(())
            }
            Err(e) => {
                let _ = self.disk_free.remove_label_values::<&str>(&[]);
                let _ = self.disk_total.remove_label_values::<&str>(&[]);
                Err(e.into())
            }
        }
    }
}

/// Refresh the queue and disk gauges now and then every [`REFRESH_INTERVAL`].
/// `queue_counts` returns `None` while no database is open; the queue gauges then keep
/// their last values.
pub fn spawn_refresh<F, Fut>(
    telemetry: &'static Telemetry,
    download_dir: PathBuf,
    queue_counts: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<Vec<(QueueState, i64)>>> + Send,
{
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(REFRESH_INTERVAL);
        let mut warned = false;
        loop {
            tick.tick().await;
            refresh_once(telemetry, &download_dir, &queue_counts, &mut warned).await;
        }
    })
}

async fn refresh_once<F, Fut>(
    telemetry: &Telemetry,
    download_dir: &Path,
    queue_counts: &F,
    warned: &mut bool,
) where
    F: Fn() -> Fut,
    Fut: Future<Output = Option<Vec<(QueueState, i64)>>>,
{
    if let Some(counts) = queue_counts().await {
        telemetry.set_queue_counts(&counts);
    }
    match telemetry.refresh_disk(download_dir) {
        Ok(()) => *warned = false,
        Err(e) => {
            if !*warned {
                tracing::warn!(
                    path = %download_dir.display(),
                    error = %e,
                    "Cannot read disk space for the download folder"
                );
                *warned = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn refreshes_queue_and_disk_on_each_tick() {
        let t: &'static Telemetry = Box::leak(Box::new(Telemetry::new()));
        let dir = tempfile::tempdir().unwrap();
        let handle = spawn_refresh(t, dir.path().to_path_buf(), || async {
            Some(vec![(QueueState::Pending, 2), (QueueState::Failed, 1)])
        });
        tokio::time::sleep(Duration::from_millis(1)).await;

        assert_eq!(
            t.sample("putio_sync_queue_items", &[("state", "pending")]),
            Some(2.0)
        );
        assert_eq!(
            t.sample("putio_sync_queue_items", &[("state", "failed")]),
            Some(1.0)
        );
        assert_eq!(
            t.sample("putio_sync_queue_items", &[("state", "paused")]),
            Some(0.0)
        );
        assert!(t.sample("putio_sync_disk_free_bytes", &[]).unwrap() > 0.0);
        assert!(t.sample("putio_sync_disk_total_bytes", &[]).unwrap() > 0.0);
        handle.abort();
    }

    #[tokio::test]
    async fn disk_error_removes_gauges_and_success_restores_them() {
        let t = Telemetry::new();
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let none = || async { None };
        let mut warned = false;

        refresh_once(&t, dir.path(), &none, &mut warned).await;
        assert!(t.sample("putio_sync_disk_free_bytes", &[]).is_some());

        refresh_once(&t, &missing, &none, &mut warned).await;
        assert!(warned);
        assert!(t.sample("putio_sync_disk_free_bytes", &[]).is_none());
        assert!(t.sample("putio_sync_disk_total_bytes", &[]).is_none());

        refresh_once(&t, dir.path(), &none, &mut warned).await;
        assert!(!warned);
        assert!(t.sample("putio_sync_disk_free_bytes", &[]).is_some());
    }

    #[tokio::test]
    async fn none_keeps_queue_gauges_at_zero() {
        let t = Telemetry::new();
        let dir = tempfile::tempdir().unwrap();
        let mut warned = false;
        refresh_once(&t, dir.path(), &|| async { None }, &mut warned).await;
        assert_eq!(
            t.sample("putio_sync_queue_items", &[("state", "pending")]),
            Some(0.0)
        );
    }
}
