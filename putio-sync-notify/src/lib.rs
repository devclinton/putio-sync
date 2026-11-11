//! Email notifications (feature 030): the chosen events from the internal event bus, sent
//! over SMTP as plain text, with repeats of one kind grouped into a summary the way the
//! bundled notifier plugins do. Settings are `[notifications.smtp]`
//! ([`putio_sync_core::email::SmtpConfig`]).

pub mod message;
pub mod smtp;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use putio_sync_core::email::SmtpConfig;
use putio_sync_core::events::{self, Event, EventType};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use message::{Grouper, Message};

/// Emails waiting to be sent; past this new ones are dropped
const QUEUE_LIMIT: usize = 100;
/// How often closed grouping windows are summarised
const TICK: Duration = Duration::from_secs(15);

/// The last email sent or tried
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delivery {
    /// When it was sent or failed
    pub at: DateTime<Utc>,
    /// Its subject, without the `putio-sync: ` prefix
    pub subject: String,
    /// Why it failed; `None` when the server took it
    pub error: Option<String>,
}

/// The email channel as the API shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmailStatus {
    /// Whether it sends
    pub enabled: bool,
    /// Emails the server took
    pub sent: u64,
    /// Emails that failed
    pub failed: u64,
    /// The last email sent or tried, the test included
    pub last: Option<Delivery>,
}

struct Inner {
    config: RwLock<SmtpConfig>,
    grouper: Mutex<Grouper>,
    host: Mutex<String>,
    status: Mutex<EmailStatus>,
    queue: mpsc::Sender<Message>,
    waiting: Mutex<Option<mpsc::Receiver<Message>>>,
    started: AtomicBool,
}

/// Sends the chosen events as email. Cheap to clone.
#[derive(Clone)]
pub struct EmailService {
    inner: Arc<Inner>,
}

fn window(config: &SmtpConfig) -> Duration {
    Duration::from_secs(u64::from(config.group_minutes) * 60)
}

impl EmailService {
    /// A service for `config` (already validated); nothing is sent until
    /// [`start`](Self::start)
    pub fn new(config: SmtpConfig) -> EmailService {
        let (queue, waiting) = mpsc::channel(QUEUE_LIMIT);
        EmailService {
            inner: Arc::new(Inner {
                grouper: Mutex::new(Grouper::new(window(&config))),
                status: Mutex::new(EmailStatus {
                    enabled: config.enabled,
                    sent: 0,
                    failed: 0,
                    last: None,
                }),
                config: RwLock::new(config),
                host: Mutex::new(String::new()),
                queue,
                waiting: Mutex::new(Some(waiting)),
                started: AtomicBool::new(false),
            }),
        }
    }

    /// Start reading the bus and sending. Needs a Tokio runtime; a second call does
    /// nothing.
    pub fn start(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(mut waiting) = self.inner.waiting.lock().expect("email queue lock").take() else {
            return;
        };
        let rx = events::subscribe();
        tokio::spawn(dispatch(self.clone(), rx));
        let ticker = self.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(TICK);
            loop {
                every.tick().await;
                ticker.tick();
            }
        });
        let sender = self.clone();
        tokio::spawn(async move {
            while let Some(message) = waiting.recv().await {
                let config = sender.config();
                if config.enabled {
                    let _ = sender.deliver(&config, &message).await;
                }
            }
        });
    }

    /// Replace the settings; open grouping windows keep their start
    pub fn configure(&self, config: SmtpConfig) {
        self.inner
            .grouper
            .lock()
            .expect("email grouper lock")
            .set_window(window(&config));
        self.status_mut().enabled = config.enabled;
        *self.inner.config.write().expect("email config lock") = config;
    }

    /// The settings in use
    pub fn config(&self) -> SmtpConfig {
        self.inner.config.read().expect("email config lock").clone()
    }

    /// Counts and the last delivery
    pub fn status(&self) -> EmailStatus {
        self.status_mut().clone()
    }

    /// Whether email is on for `event_type`
    pub fn wants(&self, event_type: EventType) -> bool {
        let config = self.inner.config.read().expect("email config lock");
        config.enabled && config.event_types().contains(&event_type)
    }

    /// Send a test email with `config` now, enabled or not, and return why it failed
    pub async fn test(&self, config: &SmtpConfig) -> Result<(), String> {
        let host = Event::new(events::Payload::HookTest(events::HookTest {
            hook: "email".to_string(),
            message: String::new(),
        }))
        .source
        .host;
        *self.inner.host.lock().expect("email host lock") = host;
        let message = Message {
            title: "Test email".to_string(),
            text: "Email notifications from putio-sync work.".to_string(),
        };
        self.deliver(config, &message).await
    }

    /// Queue the summaries of grouping windows that have closed
    pub fn tick(&self) {
        let due = self
            .inner
            .grouper
            .lock()
            .expect("email grouper lock")
            .tick(Instant::now());
        self.queue(due);
    }

    fn on_event(&self, event: &Event) {
        if !self.wants(event.event_type()) {
            return;
        }
        *self.inner.host.lock().expect("email host lock") = event.source.host.clone();
        let out = self
            .inner
            .grouper
            .lock()
            .expect("email grouper lock")
            .event(event, Instant::now());
        self.queue(out);
    }

    fn queue(&self, messages: Vec<Message>) {
        for message in messages {
            if self.inner.queue.try_send(message).is_err() {
                tracing::warn!("email queue full; dropped a notification");
            }
        }
    }

    async fn deliver(&self, config: &SmtpConfig, message: &Message) -> Result<(), String> {
        let host = self.inner.host.lock().expect("email host lock").clone();
        let body = if host.is_empty() {
            format!("{}\n\nSent by putio-sync.\n", message.text)
        } else {
            format!("{}\n\nSent by putio-sync on {}.\n", message.text, host)
        };
        let result = smtp::send(config, &message.title, &body).await;
        let mut status = self.status_mut();
        match &result {
            Ok(()) => status.sent += 1,
            Err(e) => {
                status.failed += 1;
                tracing::warn!(subject = %message.title, error = %e, "email notification failed");
            }
        }
        status.last = Some(Delivery {
            at: Utc::now(),
            subject: message.title.clone(),
            error: result.as_ref().err().cloned(),
        });
        result
    }

    fn status_mut(&self) -> std::sync::MutexGuard<'_, EmailStatus> {
        self.inner.status.lock().unwrap_or_else(|e| e.into_inner())
    }
}

async fn dispatch(service: EmailService, mut rx: broadcast::Receiver<Arc<Event>>) {
    loop {
        match rx.recv().await {
            Ok(event) => service.on_event(&event),
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "email notifications fell behind the event bus");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
