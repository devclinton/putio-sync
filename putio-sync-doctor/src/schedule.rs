//! Scheduled runs for the daemon
//!
//! The first run waits [`STARTUP_DELAY`] so it does not compete with the daemon's first
//! sync. The `[doctor]` settings are read again before every run, so a change from the
//! Settings page applies from the next one.

use crate::Doctor;
use std::sync::Arc;
use std::time::Duration;

/// Wait before the first scheduled run
pub const STARTUP_DELAY: Duration = Duration::from_secs(120);

/// Run the checks every `interval_minutes` while `[doctor] enabled`; never returns
pub async fn run_scheduled(doctor: Arc<Doctor>) {
    tokio::time::sleep(STARTUP_DELAY).await;
    loop {
        let settings = doctor.settings();
        if settings.enabled {
            doctor.run().await;
        }
        tokio::time::sleep(settings.interval()).await;
    }
}
