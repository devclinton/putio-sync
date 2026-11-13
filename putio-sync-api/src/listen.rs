//! Listening on a list of addresses (feature 037).
//!
//! A [`Supervisor`] owns one listener set (the API's or the metrics'). It resolves each
//! entry to socket addresses, binds the ones not bound yet, and closes the ones an entry
//! no longer gives. Interface and tailnet entries are checked again every second for the
//! first 5 minutes and every 30 s after; a list of fixed addresses that all bound stops
//! checking. An entry with no address waits: nothing ever falls back to another address.

use axum::{Extension, Router};
use putio_sync_core::listen::ListenAddr;
use putio_sync_tailscale::{tailnet_bind, TailscaleService};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{error, info, warn};

/// The addresses a listener set holds now, in entry order. Shared with the handlers
/// (`AppState.listening`), so they report the port actually listened on.
#[derive(Clone, Debug, Default)]
pub struct Bound(Arc<RwLock<Vec<SocketAddr>>>);

impl Bound {
    /// The bound addresses
    pub fn get(&self) -> Vec<SocketAddr> {
        self.0.read().map(|v| v.clone()).unwrap_or_default()
    }

    /// The first bound address
    pub fn first(&self) -> Option<SocketAddr> {
        self.0.read().ok().and_then(|v| v.first().copied())
    }

    fn set(&self, addrs: Vec<SocketAddr>) {
        if let Ok(mut v) = self.0.write() {
            *v = addrs;
        }
    }
}

/// The local address of the listener a request came in on (a wildcard for `0.0.0.0:P`).
/// Missing when the router is served some other way, e.g. in tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenedOn(pub SocketAddr);

/// What an entry gives now
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gives {
    /// Socket addresses to listen on
    Addrs(Vec<SocketAddr>),
    /// None now, and why
    Waiting(String),
}

/// Resolve every entry; Tailscale is asked once, and only when a tailnet entry needs it
pub async fn resolve_all(entries: &[ListenAddr], tailscale: &TailscaleService) -> Vec<Gives> {
    let status = if entries
        .iter()
        .any(|e| matches!(e, ListenAddr::Tailnet { .. }))
    {
        Some(tailscale.refresh().await)
    } else {
        None
    };
    entries
        .iter()
        .map(|entry| match entry {
            ListenAddr::Tailnet { port } => match status.as_ref().map(|s| tailnet_bind(s, *port)) {
                Some(Ok(addr)) => Gives::Addrs(vec![addr]),
                Some(Err(why)) => Gives::Waiting(why),
                None => Gives::Waiting("needs Tailscale".to_string()),
            },
            other => match other.resolve_local() {
                Ok(addrs) => Gives::Addrs(addrs),
                Err(why) => Gives::Waiting(why),
            },
        })
        .collect()
}

/// Bind one address: `SO_REUSEADDR` on unix (as tokio does) and IPv6 sockets v6-only,
/// so `[::]:P` and `0.0.0.0:P` are two separate listeners.
pub fn bind_one(addr: SocketAddr) -> std::io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    TcpListener::from_std(socket.into())
}

/// How often entries are checked again
#[derive(Clone, Copy, Debug)]
pub struct Schedule {
    /// Interval at first
    pub fast: Duration,
    /// How long the fast interval lasts, and when a waiting entry logs an error
    pub fast_for: Duration,
    /// Interval after that
    pub slow: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            fast: Duration::from_secs(1),
            fast_for: Duration::from_secs(5 * 60),
            slow: Duration::from_secs(30),
        }
    }
}

type ResolveFuture = Pin<Box<dyn Future<Output = Vec<Gives>> + Send>>;
/// Resolves the entries; the test seam
pub type Resolver = Arc<dyn Fn(Vec<ListenAddr>) -> ResolveFuture + Send + Sync>;

/// A running listener set. Dropping it leaves the set running for the life of the
/// process; [`ListenerHandle::stop`] closes it.
pub struct ListenerHandle {
    /// What it listens on now
    pub bound: Bound,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ListenerHandle {
    /// Wait up to `timeout` for a first bound address
    pub async fn first_bound(&self, timeout: Duration) -> Option<SocketAddr> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(addr) = self.bound.first() {
                return Some(addr);
            }
            if Instant::now() >= until {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Stop accepting on every address; open connections get `grace` to finish
    pub async fn stop(self, grace: Duration) {
        let _ = self.stop.send(true);
        let mut task = self.task;
        if tokio::time::timeout(grace + Duration::from_secs(1), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
        self.bound.set(Vec::new());
    }
}

/// Starts listener sets
pub struct Supervisor;

impl Supervisor {
    /// Listen on `entries` with `router`, resolving tailnet entries through `tailscale`.
    /// `name` ("API", "Metrics") starts every log line.
    pub fn spawn(
        name: &'static str,
        entries: Vec<ListenAddr>,
        tailscale: Arc<TailscaleService>,
        router: Router,
        bound: Bound,
    ) -> ListenerHandle {
        let resolver: Resolver = Arc::new(move |entries| {
            let tailscale = tailscale.clone();
            Box::pin(async move { resolve_all(&entries, &tailscale).await })
        });
        Self::spawn_with(name, entries, resolver, router, bound, Schedule::default())
    }

    /// [`Supervisor::spawn`] with its resolver and schedule given
    pub fn spawn_with(
        name: &'static str,
        entries: Vec<ListenAddr>,
        resolver: Resolver,
        router: Router,
        bound: Bound,
        schedule: Schedule,
    ) -> ListenerHandle {
        let (stop, stopped) = watch::channel(false);
        let run = Run {
            name,
            entries,
            resolver,
            router,
            bound: bound.clone(),
            schedule,
        };
        let task = tokio::spawn(run.run(stopped));
        ListenerHandle { bound, stop, task }
    }
}

/// One bound address being served
struct Serving {
    local: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

struct Run {
    name: &'static str,
    entries: Vec<ListenAddr>,
    resolver: Resolver,
    router: Router,
    bound: Bound,
    schedule: Schedule,
}

impl Run {
    async fn run(self, mut stopped: watch::Receiver<bool>) {
        let name = self.name;
        let started = Instant::now();
        let dynamic = self.entries.iter().any(ListenAddr::is_dynamic);
        let mut serving: HashMap<SocketAddr, Serving> = HashMap::new();
        let mut waiting: Vec<Option<String>> = vec![None; self.entries.len()];
        let mut bind_errors: HashMap<SocketAddr, String> = HashMap::new();
        let mut long_wait_logged = false;

        loop {
            let gives = (self.resolver)(self.entries.clone()).await;
            let mut wanted: Vec<SocketAddr> = Vec::new();
            for (i, (entry, g)) in self.entries.iter().zip(&gives).enumerate() {
                match g {
                    Gives::Addrs(addrs) => {
                        if waiting[i].take().is_some() {
                            info!(listener = name, entry = %entry, "{name}: {entry} has an address now");
                        }
                        for a in addrs {
                            if !wanted.contains(a) {
                                wanted.push(*a);
                            }
                        }
                    }
                    Gives::Waiting(why) => {
                        if waiting[i].as_deref() != Some(why) {
                            warn!(listener = name, entry = %entry, reason = %why, "{name}: waiting for {entry} to have an address; not listening there");
                            waiting[i] = Some(why.clone());
                        }
                    }
                }
            }

            // Close what no entry gives any more
            let gone: Vec<SocketAddr> = serving
                .keys()
                .filter(|a| !wanted.contains(a))
                .copied()
                .collect();
            for addr in gone {
                if let Some(s) = serving.remove(&addr) {
                    let _ = s.stop.send(true);
                    info!(listener = name, addr = %s.local, "{name}: stopped listening on {}", s.local);
                }
            }
            // Bind what is new
            for addr in &wanted {
                if serving.contains_key(addr) {
                    continue;
                }
                match bind_one(*addr) {
                    Ok(listener) => {
                        bind_errors.remove(addr);
                        let local = listener.local_addr().unwrap_or(*addr);
                        serving.insert(*addr, serve(listener, local, self.router.clone()));
                        info!(listener = name, addr = %local, "{name} listening on {local}");
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if bind_errors.get(addr) != Some(&msg) {
                            error!(listener = name, addr = %addr, error = %msg, "{name}: can't listen on {addr}; trying again");
                            bind_errors.insert(*addr, msg);
                        }
                    }
                }
            }
            self.bound.set(
                wanted
                    .iter()
                    .filter_map(|a| serving.get(a).map(|s| s.local))
                    .collect(),
            );

            let unbound = wanted.len() > serving.len() || waiting.iter().any(Option::is_some);
            if unbound && !long_wait_logged && started.elapsed() >= self.schedule.fast_for {
                long_wait_logged = true;
                let entries: Vec<String> = self
                    .entries
                    .iter()
                    .zip(&waiting)
                    .filter(|(_, w)| w.is_some())
                    .map(|(e, _)| e.to_string())
                    .chain(
                        wanted
                            .iter()
                            .filter(|a| !serving.contains_key(a))
                            .map(|a| a.to_string()),
                    )
                    .collect();
                error!(listener = name, waiting = %entries.join(", "), "{name}: still not listening on {} after {} s; checking every {} s", entries.join(", "), self.schedule.fast_for.as_secs(), self.schedule.slow.as_secs());
            }

            if !dynamic && !unbound {
                // Fixed addresses, all bound: nothing will change until stopped
                stop_asked(&mut stopped).await;
                break;
            }
            let tick = if started.elapsed() < self.schedule.fast_for {
                self.schedule.fast
            } else {
                self.schedule.slow
            };
            tokio::select! {
                _ = tokio::time::sleep(tick) => {}
                _ = stop_asked(&mut stopped) => break,
            }
        }

        for s in serving.values() {
            let _ = s.stop.send(true);
        }
        for (_, s) in serving {
            let _ = s.task.await;
        }
    }
}

/// Resolves once a stop is sent; never when the handle was dropped without one
async fn stop_asked(stopped: &mut watch::Receiver<bool>) {
    if stopped.wait_for(|s| *s).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Serve `router` on `listener`, telling handlers which listener it is, until stopped
fn serve(listener: TcpListener, local: SocketAddr, router: Router) -> Serving {
    let (stop, mut stopped) = watch::channel(false);
    let app = router
        .layer(Extension(ListenedOn(local)))
        .into_make_service_with_connect_info::<SocketAddr>();
    let task = tokio::spawn(async move {
        let shutdown = async move {
            let _ = stopped.wait_for(|s| *s).await;
        };
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
        {
            error!(addr = %local, error = %e, "Listener on {local} stopped");
        }
    });
    Serving { local, stop, task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn app() -> Router {
        Router::new().route(
            "/on",
            get(|Extension(ListenedOn(a)): Extension<ListenedOn>| async move { a.to_string() }),
        )
    }

    fn fast() -> Schedule {
        Schedule {
            fast: Duration::from_millis(20),
            fast_for: Duration::from_secs(60),
            slow: Duration::from_millis(20),
        }
    }

    fn entry(s: &str) -> ListenAddr {
        s.parse().unwrap()
    }

    fn local_resolver() -> Resolver {
        Arc::new(|entries: Vec<ListenAddr>| {
            Box::pin(async move {
                entries
                    .iter()
                    .map(|e| match e.resolve_local() {
                        Ok(a) => Gives::Addrs(a),
                        Err(w) => Gives::Waiting(w),
                    })
                    .collect()
            })
        })
    }

    async fn get_on(addr: SocketAddr) -> Option<String> {
        let resp = reqwest::Client::new()
            .get(format!("http://{addr}/on"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .ok()?;
        resp.text().await.ok()
    }

    async fn until(mut f: impl FnMut() -> bool) {
        for _ in 0..200 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("condition not met in 4 s");
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn binds_every_entry_and_tells_handlers_which() {
        let mut entries = vec![entry("127.0.0.1:0")];
        let v6 = std::net::TcpListener::bind("[::1]:0").is_ok();
        if v6 {
            entries.push(entry("[::1]:0"));
        }
        let h = Supervisor::spawn_with(
            "Test",
            entries,
            local_resolver(),
            app(),
            Bound::default(),
            fast(),
        );
        h.first_bound(Duration::from_secs(2)).await.unwrap();
        until(|| h.bound.get().len() == if v6 { 2 } else { 1 }).await;
        for addr in h.bound.get() {
            assert_eq!(get_on(addr).await.unwrap(), addr.to_string());
        }
        h.stop(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn a_busy_port_is_retried_while_others_serve() {
        let port = free_port();
        let busy = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        let h = Supervisor::spawn_with(
            "Test",
            vec![entry("127.0.0.1:0"), entry(&format!("127.0.0.1:{port}"))],
            local_resolver(),
            app(),
            Bound::default(),
            fast(),
        );
        let first = h.first_bound(Duration::from_secs(2)).await.unwrap();
        assert!(get_on(first).await.is_some());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.bound.get().len(), 1);
        drop(busy);
        until(|| h.bound.get().len() == 2).await;
        assert_eq!(h.bound.get()[1].port(), port);
        h.stop(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn follows_a_changing_resolution_and_never_binds_unnamed_addresses() {
        let port = free_port();
        let up = Arc::new(AtomicBool::new(false));
        let flag = up.clone();
        let resolver: Resolver = Arc::new(move |_| {
            let up = flag.load(Ordering::SeqCst);
            Box::pin(async move {
                vec![if up {
                    Gives::Addrs(vec![SocketAddr::from(([127, 0, 0, 1], port))])
                } else {
                    Gives::Waiting("no such interface eth9".to_string())
                }]
            })
        });
        let h = Supervisor::spawn_with(
            "Test",
            vec![entry(&format!("eth9:{port}"))],
            resolver,
            app(),
            Bound::default(),
            fast(),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(h.bound.get().is_empty());
        assert!(get_on(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .is_none());

        up.store(true, Ordering::SeqCst);
        until(|| h.bound.get().len() == 1).await;
        assert!(get_on(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .is_some());

        up.store(false, Ordering::SeqCst);
        until(|| h.bound.get().is_empty()).await;
        let mut closed = false;
        for _ in 0..50 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                closed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(closed, "the port was not freed");
        h.stop(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn loopback_interface_by_name_binds_and_a_missing_one_waits() {
        let Some(lo) = putio_sync_core::listen::interface_of([127, 0, 0, 1].into()) else {
            return;
        };
        let port = free_port();
        // Windows names it "Loopback Pseudo-Interface 1", which a setting can't name yet
        let Ok(lo) = format!("{lo}:{port}").parse::<ListenAddr>() else {
            return;
        };
        let h = Supervisor::spawn_with(
            "Test",
            vec![lo, entry(&format!("nope-037:{port}"))],
            local_resolver(),
            app(),
            Bound::default(),
            fast(),
        );
        until(|| {
            h.bound
                .get()
                .contains(&SocketAddr::from(([127, 0, 0, 1], port)))
        })
        .await;
        assert!(get_on(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .is_some());
        h.stop(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn stop_frees_the_port() {
        let port = free_port();
        let h = Supervisor::spawn_with(
            "Test",
            vec![entry(&format!("127.0.0.1:{port}"))],
            local_resolver(),
            app(),
            Bound::default(),
            fast(),
        );
        h.first_bound(Duration::from_secs(2)).await.unwrap();
        let bound = h.bound.clone();
        h.stop(Duration::from_secs(1)).await;
        assert!(bound.get().is_empty());
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[tokio::test]
    async fn ipv6_wildcard_and_ipv4_wildcard_share_a_port() {
        if std::net::TcpListener::bind("[::1]:0").is_err() {
            return;
        }
        let port = free_port();
        let v4 = bind_one(SocketAddr::from(([0, 0, 0, 0], port))).unwrap();
        let v6 = bind_one(format!("[::]:{port}").parse().unwrap());
        assert!(v6.is_ok(), "{v6:?}");
        drop(v4);
    }
}
