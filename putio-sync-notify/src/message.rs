//! What an email says for each event, and grouping of repeats. The same text and rules as
//! the bundled notifier plugins (`putio-sync-plugins/bundled/notifier.lua`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use putio_sync_core::events::{Event, EventType, Payload};

/// Names listed in a summary; the rest are counted
const NAMES_KEPT: usize = 5;

/// One message to send
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The subject, without the `putio-sync: ` prefix
    pub title: String,
    /// The body's first line
    pub text: String,
}

/// `1.4 GB`, as the notifiers write sizes
pub fn size(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

fn with_size(name: &str, bytes: Option<i64>) -> String {
    match bytes {
        Some(b) => format!("{} ({})", name, size(b)),
        None => name.to_string(),
    }
}

fn with_error(name: &str, error: Option<&str>) -> String {
    match error.filter(|e| !e.is_empty()) {
        Some(e) => format!("{}: {}", name, e),
        None => name.to_string(),
    }
}

/// The message for `event`, or `None` for events that send nothing (a successful sync)
pub fn message(event: &Event) -> Option<Message> {
    let (title, text) = match &event.payload {
        Payload::DownloadFinished(d) => {
            ("Downloaded", with_size(&d.name, Some(d.size_bytes as i64)))
        }
        Payload::DownloadFailed(d) => ("Download failed", with_error(&d.name, Some(&d.error))),
        Payload::TransferAdded(t) => ("Transfer added", t.name.clone()),
        Payload::TransferFinished(t) => ("Transfer finished", with_size(&t.name, t.size_bytes)),
        Payload::TransferFailed(t) => ("Transfer failed", with_error(&t.name, t.error.as_deref())),
        Payload::TransferStuck(t) => (
            "Transfer stuck",
            format!("{}: {} at {}%", t.name, t.reason, t.percent_done),
        ),
        Payload::SyncCompleted(s) if s.result == "failure" => (
            "Sync failed",
            s.error
                .clone()
                .unwrap_or_else(|| "The sync run failed".to_string()),
        ),
        Payload::StorageLow(s) => (
            "Put.io storage low",
            format!(
                "{:.1}% free ({} of {})",
                s.free_percent,
                size(s.free_bytes),
                size(s.total_bytes)
            ),
        ),
        Payload::DoctorProblem(d) => {
            return Some(Message {
                title: format!("Doctor: {}", d.title),
                text: d.summary.clone(),
            })
        }
        Payload::VerifyFailed(v) => ("Verify failed", v.name.clone()),
        Payload::PipelineStepFailed(p) => (
            "Processing step failed",
            format!("{} ({}): {}", p.name, p.step, p.error),
        ),
        _ => return None,
    };
    Some(Message {
        title: title.to_string(),
        text,
    })
}

/// The name a summary lists for `event`
fn name_of(event: &Event) -> Option<String> {
    let name = match &event.payload {
        Payload::DownloadFinished(d) => &d.name,
        Payload::DownloadFailed(d) => &d.name,
        Payload::TransferAdded(t) | Payload::TransferFinished(t) | Payload::TransferFailed(t) => {
            &t.name
        }
        Payload::TransferStuck(t) => &t.name,
        Payload::DoctorProblem(d) => &d.title,
        Payload::VerifyFailed(v) => &v.name,
        Payload::PipelineStepFailed(p) => &p.name,
        _ => return None,
    };
    Some(name.clone())
}

/// The noun of a summary: `3 more downloads finished`
fn summary_noun(t: EventType) -> &'static str {
    match t {
        EventType::DownloadFinished => "downloads finished",
        EventType::DownloadFailed => "downloads failed",
        EventType::TransferAdded => "transfers added",
        EventType::TransferFinished => "transfers finished",
        EventType::TransferFailed => "transfers failed",
        EventType::TransferStuck => "transfers stuck",
        EventType::SyncCompleted => "sync runs failed",
        EventType::StorageLow => "low storage warnings",
        EventType::DoctorProblem => "doctor problems",
        EventType::VerifyFailed => "files failed verification",
        EventType::PipelineStepFailed => "processing steps failed",
        other => other.as_str(),
    }
}

struct Group {
    opened: Instant,
    count: usize,
    names: Vec<String>,
}

/// Groups repeats: the first event of a type is sent at once and opens a window; later
/// ones in the window are counted, and one summary is sent once it closes.
pub struct Grouper {
    window: Duration,
    groups: HashMap<EventType, Group>,
}

impl Grouper {
    /// Group for `window`; zero sends every event
    pub fn new(window: Duration) -> Grouper {
        Grouper {
            window,
            groups: HashMap::new(),
        }
    }

    /// Change the window; open groups keep their start
    pub fn set_window(&mut self, window: Duration) {
        self.window = window;
    }

    /// What to send for `event` at `now`: a summary of a closed window first, if any,
    /// then the event's own message unless a window is open for its type
    pub fn event(&mut self, event: &Event, now: Instant) -> Vec<Message> {
        let Some(own) = message(event) else {
            return Vec::new();
        };
        let t = event.event_type();
        let mut out = Vec::new();
        if let Some(g) = self.groups.get(&t) {
            if now.duration_since(g.opened) >= self.window {
                out.extend(self.flush(t));
            }
        }
        match self.groups.get_mut(&t) {
            Some(g) => {
                g.count += 1;
                if let Some(name) = name_of(event).filter(|_| g.names.len() < NAMES_KEPT) {
                    g.names.push(name);
                }
            }
            None => {
                if !self.window.is_zero() {
                    self.groups.insert(
                        t,
                        Group {
                            opened: now,
                            count: 0,
                            names: Vec::new(),
                        },
                    );
                }
                out.push(own);
            }
        }
        out
    }

    /// Summaries of every window that has closed by `now`
    pub fn tick(&mut self, now: Instant) -> Vec<Message> {
        let closed: Vec<EventType> = self
            .groups
            .iter()
            .filter(|(_, g)| now.duration_since(g.opened) >= self.window)
            .map(|(t, _)| *t)
            .collect();
        closed.into_iter().filter_map(|t| self.flush(t)).collect()
    }

    fn flush(&mut self, t: EventType) -> Option<Message> {
        let g = self.groups.remove(&t)?;
        if g.count == 0 {
            return None;
        }
        let mut text = g.names.join(", ");
        let rest = g.count - g.names.len();
        if rest > 0 {
            if !text.is_empty() {
                text.push_str(" and ");
            }
            text.push_str(&format!("{} more", rest));
        }
        Some(Message {
            title: format!("{} more {}", g.count, summary_noun(t)),
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::events::samples;

    fn sample(t: EventType) -> Event {
        Event::new(samples().into_iter().find(|p| p.event_type() == t).unwrap())
    }

    fn finished(name: &str) -> Event {
        let mut e = sample(EventType::DownloadFinished);
        if let Payload::DownloadFinished(d) = &mut e.payload {
            d.name = name.to_string();
        }
        e
    }

    #[test]
    fn messages_match_the_notifier_plugins() {
        let text = |t| message(&sample(t)).map(|m| (m.title, m.text));
        assert_eq!(
            text(EventType::DownloadFinished),
            Some(("Downloaded".into(), "episode.mkv (1.4 GB)".into()))
        );
        assert_eq!(
            text(EventType::TransferStuck),
            Some((
                "Transfer stuck".into(),
                "Some.Show.S01E02.1080p: no peers at 37%".into()
            ))
        );
        assert_eq!(
            text(EventType::DoctorProblem),
            Some((
                "Doctor: Disk space".into(),
                "2.1 GB free on /data, below the 5 GB minimum".into()
            ))
        );
        assert_eq!(
            text(EventType::StorageLow),
            Some((
                "Put.io storage low".into(),
                "5.0% free (5.0 GB of 100.0 GB)".into()
            ))
        );
        // A successful sync sends nothing
        assert_eq!(text(EventType::SyncCompleted), None);
        assert_eq!(size(999), "999 B");
    }

    #[test]
    fn repeats_in_the_window_become_one_summary() {
        let start = Instant::now();
        let mut g = Grouper::new(Duration::from_secs(300));
        let mut sent = Vec::new();
        for i in 0..10 {
            sent.extend(g.event(
                &finished(&format!("File {i}")),
                start + Duration::from_secs(i),
            ));
        }
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "File 0 (1.4 GB)");
        assert!(g.tick(start + Duration::from_secs(299)).is_empty());
        let summary = g.tick(start + Duration::from_secs(300));
        assert_eq!(
            summary,
            vec![Message {
                title: "9 more downloads finished".into(),
                text: "File 1, File 2, File 3, File 4, File 5 and 4 more".into()
            }]
        );
        // A new window opens with the next event
        let next = g.event(&finished("File 10"), start + Duration::from_secs(400));
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn an_event_after_the_window_sends_the_summary_first() {
        let start = Instant::now();
        let mut g = Grouper::new(Duration::from_secs(60));
        g.event(&finished("a"), start);
        g.event(&finished("b"), start + Duration::from_secs(1));
        let out = g.event(&finished("c"), start + Duration::from_secs(61));
        let titles: Vec<_> = out.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, vec!["1 more downloads finished", "Downloaded"]);
    }

    #[test]
    fn zero_minutes_sends_each_and_types_group_apart() {
        let now = Instant::now();
        let mut g = Grouper::new(Duration::ZERO);
        assert_eq!(g.event(&finished("a"), now).len(), 1);
        assert_eq!(g.event(&finished("b"), now).len(), 1);
        let mut g = Grouper::new(Duration::from_secs(60));
        assert_eq!(g.event(&finished("a"), now).len(), 1);
        assert_eq!(g.event(&sample(EventType::DoctorProblem), now).len(), 1);
    }
}
