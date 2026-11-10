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
        match disk_usage(dir) {
            Ok((free, total)) => {
                self.disk_free
                    .with_label_values::<&str>(&[])
                    .set(free as f64);
                self.disk_total
                    .with_label_values::<&str>(&[])
                    .set(total as f64);
                Ok(())
            }
            Err(e) => {
                let _ = self.disk_free.remove_label_values::<&str>(&[]);
                let _ = self.disk_total.remove_label_values::<&str>(&[]);
                Err(e)
            }
        }
    }
}

/// `(free, total)` bytes of the filesystem holding `dir`, from `statvfs`. Free is the
/// space available to unprivileged users (`f_bavail`), as `df` reports it.
#[cfg(unix)]
pub fn disk_usage(dir: &Path) -> std::io::Result<(u64, u64)> {
    let st = rustix::fs::statvfs(dir)?;
    let frsize = st.f_frsize;
    Ok((
        st.f_bavail.saturating_mul(frsize),
        st.f_blocks.saturating_mul(frsize),
    ))
}

/// `(free, total)` bytes of the volume holding `dir`, from `GetDiskFreeSpaceExW`. Free
/// is what the calling user may use (quotas applied), like `f_bavail`.
#[cfg(windows)]
pub fn disk_usage(dir: &Path) -> std::io::Result<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointers are valid.
    let ok =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((free, total))
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

    #[test]
    fn disk_usage_reports_free_and_total() {
        let dir = tempfile::tempdir().unwrap();
        let (free, total) = disk_usage(dir.path()).unwrap();
        assert!(free > 0);
        assert!(total >= free);
        assert!(disk_usage(&dir.path().join("missing")).is_err());
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
