//! The rules that stop every download at once (feature 064): download windows and the
//! free-space pause.
//!
//! The shared [`RateLimiter`](crate::RateLimiter) asks [`Gates::check`] about once a
//! second and treats a hold as a pause, so no worker claims a row and running downloads
//! wait in place, the way they do for a paused bandwidth schedule. The last answer is
//! kept for the reason line on the queue and Active pages.

use chrono::NaiveDateTime;
use putio_sync_core::download_window::{space_paused, State, Windows};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::{info, warn};

/// Why every download is stopped
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// Outside every download window
    Window {
        /// When the next window starts, on the daemon's wall clock
        reopens: Option<NaiveDateTime>,
    },
    /// Free space fell below the pause level and is not back at the resume level
    Space {
        free_bytes: u64,
        pause_bytes: u64,
        resume_bytes: u64,
    },
}

/// The settings behind the gates
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    /// `[[download.windows]]`
    pub windows: Windows,
    /// `download.pause_free_space_mb` in bytes; 0 is off
    pub pause_bytes: u64,
    /// The free space that ends the pause, in bytes
    pub resume_bytes: u64,
    /// The download folder, whose volume is measured
    pub dir: PathBuf,
}

/// Reads the free bytes of a folder's volume; `None` when it can't be read
pub type FreeSpace = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

struct Inner {
    rules: Rules,
    space_paused: bool,
    last_free: u64,
    hold: Option<Hold>,
}

/// The windows and the free-space pause, with the pause's memory
pub struct Gates {
    inner: Mutex<Inner>,
    free: FreeSpace,
}

impl std::fmt::Debug for Gates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gates").field("hold", &self.hold()).finish()
    }
}

impl Gates {
    /// Gates following `rules`, measuring free space with `free`
    pub fn new(rules: Rules, free: FreeSpace) -> Self {
        Self {
            inner: Mutex::new(Inner {
                rules,
                space_paused: false,
                last_free: 0,
                hold: None,
            }),
            free,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Take new settings; the next check applies them. A pause in force stays until
    /// free space is at the (new) resume level, or the pause is turned off.
    pub fn set_rules(&self, rules: Rules) {
        self.lock().rules = rules;
    }

    /// What stops every download at wall-clock time `local`, if anything. A window
    /// comes before the free-space pause. Logs when a hold begins and ends.
    pub fn check(&self, local: NaiveDateTime) -> Option<Hold> {
        let mut inner = self.lock();
        let window = match inner.rules.windows.state(local) {
            State::Open => None,
            State::Closed { reopens } => Some(Hold::Window { reopens }),
        };
        let (pause, resume) = (inner.rules.pause_bytes, inner.rules.resume_bytes);
        let free = if pause > 0 {
            (self.free)(&inner.rules.dir)
        } else {
            None
        };
        if let Some(free) = free {
            inner.last_free = free;
        }
        let was_paused = inner.space_paused;
        inner.space_paused = space_paused(was_paused, free, pause, resume);
        match (was_paused, inner.space_paused) {
            (false, true) => warn!(
                free_bytes = inner.last_free,
                pause_bytes = pause,
                resume_bytes = resume,
                "Low disk space: every download is paused until free space is back"
            ),
            (true, false) => info!(
                free_bytes = inner.last_free,
                "Enough free space again: downloads are no longer paused"
            ),
            _ => {}
        }
        let space = inner.space_paused.then_some(Hold::Space {
            free_bytes: inner.last_free,
            pause_bytes: pause,
            resume_bytes: resume.max(pause),
        });
        let was_closed = matches!(inner.hold, Some(Hold::Window { .. }));
        match (was_closed, window) {
            (false, Some(Hold::Window { reopens })) => info!(
                reopens = %reopens.map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default(),
                "Outside the download windows: downloads wait"
            ),
            (true, None) => info!("Inside a download window: downloads run"),
            _ => {}
        }
        inner.hold = window.or(space);
        inner.hold
    }

    /// The answer of the last [`check`](Self::check)
    pub fn hold(&self) -> Option<Hold> {
        self.lock().hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use putio_sync_core::download_window::WindowEntry;
    use std::sync::atomic::{AtomicU64, Ordering};

    const GB: u64 = 1 << 30;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        // A Monday
        NaiveDate::from_ymd_opt(2026, 11, 2)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn nights() -> Windows {
        Windows::from_entries(&[WindowEntry {
            days: "daily".into(),
            start: "01:00".into(),
            end: "07:00".into(),
        }])
    }

    /// Gates over a free space the test sets; `u64::MAX` reads as unreadable
    fn gates(rules: Rules) -> (Gates, Arc<AtomicU64>) {
        let free = Arc::new(AtomicU64::new(100 * GB));
        let read = free.clone();
        let gates = Gates::new(
            rules,
            Arc::new(move |_: &Path| {
                Some(read.load(Ordering::SeqCst)).filter(|free| *free != u64::MAX)
            }),
        );
        (gates, free)
    }

    #[test]
    fn no_rules_hold_nothing() {
        let (g, free) = gates(Rules::default());
        free.store(0, Ordering::SeqCst);
        assert_eq!(g.check(at(12, 0)), None);
        assert_eq!(g.hold(), None);
    }

    #[test]
    fn outside_a_window_holds_and_says_when_it_opens() {
        let (g, _) = gates(Rules {
            windows: nights(),
            ..Rules::default()
        });
        assert_eq!(g.check(at(3, 0)), None);
        let closed = g.check(at(7, 0));
        assert_eq!(
            closed,
            Some(Hold::Window {
                reopens: Some(at(1, 0) + chrono::Duration::days(1))
            })
        );
        assert_eq!(g.hold(), closed);
    }

    #[test]
    fn the_space_pause_resumes_only_at_the_higher_level() {
        let (g, free) = gates(Rules {
            pause_bytes: 10 * GB,
            resume_bytes: 15 * GB,
            ..Rules::default()
        });
        assert_eq!(g.check(at(12, 0)), None);
        free.store(9 * GB, Ordering::SeqCst);
        let hold = |free_bytes| {
            Some(Hold::Space {
                free_bytes,
                pause_bytes: 10 * GB,
                resume_bytes: 15 * GB,
            })
        };
        assert_eq!(g.check(at(12, 1)), hold(9 * GB));
        // Above the pause level but under the resume level: still paused
        free.store(12 * GB, Ordering::SeqCst);
        assert_eq!(g.check(at(12, 2)), hold(12 * GB));
        // Unreadable: the pause stays, with the last reading
        free.store(u64::MAX, Ordering::SeqCst);
        assert_eq!(g.check(at(12, 3)), hold(12 * GB));
        free.store(15 * GB, Ordering::SeqCst);
        assert_eq!(g.check(at(12, 4)), None);
        // And it does not start again between the levels
        free.store(12 * GB, Ordering::SeqCst);
        assert_eq!(g.check(at(12, 5)), None);
    }

    #[test]
    fn new_rules_apply_at_the_next_check_and_turning_the_pause_off_ends_it() {
        let (g, free) = gates(Rules {
            pause_bytes: 10 * GB,
            resume_bytes: 15 * GB,
            ..Rules::default()
        });
        free.store(GB, Ordering::SeqCst);
        assert!(matches!(g.check(at(12, 0)), Some(Hold::Space { .. })));
        g.set_rules(Rules {
            windows: nights(),
            ..Rules::default()
        });
        // The pause is off; the window now holds
        assert!(matches!(g.check(at(12, 1)), Some(Hold::Window { .. })));
        assert_eq!(g.check(at(2, 0)), None);
    }

    #[test]
    fn a_window_is_named_before_the_space_pause() {
        let (g, free) = gates(Rules {
            windows: nights(),
            pause_bytes: 10 * GB,
            resume_bytes: 15 * GB,
            ..Rules::default()
        });
        free.store(GB, Ordering::SeqCst);
        assert!(matches!(g.check(at(12, 0)), Some(Hold::Window { .. })));
        assert!(matches!(g.check(at(2, 0)), Some(Hold::Space { .. })));
    }
}
