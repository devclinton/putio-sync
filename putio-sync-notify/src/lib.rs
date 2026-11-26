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
/// Waits before each new try of an email the server did not take: a mail server that is
/// restarting or briefly unreachable still gets the message
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(120),
    Duration::from_secs(600),
];

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
    /// Woken when a grouping window opens, so the ticker can sleep while none is
    window_opened: tokio::sync::Notify,
    /// [`RETRY_DELAYS`]; shorter in tests
    retry_delays: RwLock<Vec<Duration>>,
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
                window_opened: tokio::sync::Notify::new(),
                retry_delays: RwLock::new(RETRY_DELAYS.to_vec()),
            }),
        }
    }

    /// Start reading the bus and sending. Needs a Tokio runtime; a second call does
    /// nothing.
    pub fn start(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(mut waiting) = self
            .inner
            .waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        let rx = events::subscribe();
        tokio::spawn(dispatch(self.clone(), rx));
        let ticker = self.clone();
        tokio::spawn(async move {
            loop {
                // With no window open there is nothing to close: sleep until one opens
                if !ticker.has_open_window() {
                    ticker.inner.window_opened.notified().await;
                }
                tokio::time::sleep(TICK).await;
                ticker.tick();
            }
        });
        let sender = self.clone();
        tokio::spawn(async move {
            while let Some(message) = waiting.recv().await {
                sender.deliver_with_retries(&message).await;
            }
        });
    }

    /// Wait `delays` between tries instead of 30 s, 2 min and 10 min. For tests.
    #[doc(hidden)]
    pub fn set_retry_delays(&self, delays: Vec<Duration>) {
        *self
            .inner
            .retry_delays
            .write()
            .unwrap_or_else(|e| e.into_inner()) = delays;
    }

    /// Send `message`, trying again after each of the retry delays while it fails. The
    /// settings are read before each try, so turning email off stops the retries.
    async fn deliver_with_retries(&self, message: &Message) {
        let delays = self
            .inner
            .retry_delays
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut delays = delays.into_iter();
        loop {
            let config = self.config();
            if !config.enabled {
                return;
            }
            let last = delays.len() == 0;
            match self.attempt(&config, message).await {
                Ok(()) => return self.record(message, Ok(())),
                Err(e) if last => return self.record(message, Err(e)),
                Err(e) => {
                    let wait = delays.next().unwrap_or_default();
                    tracing::info!(
                        subject = %message.title,
                        error = %e,
                        retry_in_secs = wait.as_secs(),
                        "email notification failed; will try again"
                    );
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    /// Replace the settings; open grouping windows keep their start
    pub fn configure(&self, config: SmtpConfig) {
        self.inner
            .grouper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_window(window(&config));
        self.status_mut().enabled = config.enabled;
        *self.inner.config.write().unwrap_or_else(|e| e.into_inner()) = config;
    }

    /// The settings in use
    pub fn config(&self) -> SmtpConfig {
        self.inner
            .config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Counts and the last delivery
    pub fn status(&self) -> EmailStatus {
        self.status_mut().clone()
    }

    /// Whether email is on for `event_type`
    pub fn wants(&self, event_type: EventType) -> bool {
        let config = self.inner.config.read().unwrap_or_else(|e| e.into_inner());
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
        *self.inner.host.lock().unwrap_or_else(|e| e.into_inner()) = host;
        let message = Message {
            title: "Test email".to_string(),
            text: "Email notifications from putio-sync work.".to_string(),
        };
        self.deliver(config, &message).await
    }

    fn has_open_window(&self) -> bool {
        self.inner
            .grouper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .has_open()
    }

    /// Queue the summaries of grouping windows that have closed
    pub fn tick(&self) {
        let due = self
            .inner
            .grouper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tick(Instant::now());
        self.queue(due);
    }

    fn on_event(&self, event: &Event) {
        if !self.wants(event.event_type()) {
            return;
        }
        *self.inner.host.lock().unwrap_or_else(|e| e.into_inner()) = event.source.host.clone();
        let out = self
            .inner
            .grouper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .event(event, Instant::now());
        self.inner.window_opened.notify_one();
        self.queue(out);
    }

    fn queue(&self, messages: Vec<Message>) {
        for message in messages {
            if self.inner.queue.try_send(message).is_err() {
                tracing::warn!("email queue full; dropped a notification");
            }
        }
    }

    /// Send once and record the outcome
    async fn deliver(&self, config: &SmtpConfig, message: &Message) -> Result<(), String> {
        let result = self.attempt(config, message).await;
        self.record(message, result.clone());
        result
    }

    /// One try, not recorded
    async fn attempt(&self, config: &SmtpConfig, message: &Message) -> Result<(), String> {
        let host = self
            .inner
            .host
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let body = if host.is_empty() {
            format!("{}\n\nSent by putio-sync.\n", message.text)
        } else {
            format!("{}\n\nSent by putio-sync on {}.\n", message.text, host)
        };
        smtp::send(config, &message.title, &body).await
    }

    /// Count a sent or failed email and keep it as the last delivery
    fn record(&self, message: &Message, result: Result<(), String>) {
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
            error: result.err(),
        });
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
