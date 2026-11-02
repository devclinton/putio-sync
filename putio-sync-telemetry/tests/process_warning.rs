//! Contract test for `specs/041-resource-telemetry-archive/contracts/metrics.md`,
//! invariant 3. It has a binary of its own: tracing caches whether a callsite is wanted,
//! so a test on another thread without a subscriber can hide the warning from this one.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use putio_sync_telemetry::*;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

const UNAVAILABLE_MSG: &str = "Process metrics unavailable";

struct CountUnavailableWarns(Arc<AtomicUsize>);

struct MessageVisitor(bool);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" && format!("{value:?}") == UNAVAILABLE_MSG {
            self.0 = true;
        }
    }
}

impl<S: Subscriber> Layer<S> for CountUnavailableWarns {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::WARN {
            return;
        }
        let mut visitor = MessageVisitor(false);
        event.record(&mut visitor);
        if visitor.0 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn other_platforms_have_no_figures_and_warn_once() {
    let warns = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(CountUnavailableWarns(warns.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);

    let t = Telemetry::new();
    t.init(1, "1.2.3", "abc1234");
    for _ in 0..3 {
        assert!(!t.encode().contains("putio_sync_process_"));
    }
    assert_eq!(warns.load(Ordering::SeqCst), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_warns_only_for_figures_it_cannot_read() {
    let warns = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(CountUnavailableWarns(warns.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);

    let t = Telemetry::new();
    for _ in 0..3 {
        t.encode();
    }
    // 0 on a full /proc, 1 where the kernel hides something such as /proc/self/io.
    assert!(warns.load(Ordering::SeqCst) <= 1);
}
