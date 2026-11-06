//! A tiny OpenAI-style chat server for the engine tests. It counts requests and how many
//! are in flight at once, which mockito cannot show.

#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What the server answers: status and body
pub type Answer = (u16, String);
type Handler = dyn Fn(&str, usize) -> Answer + Send + Sync;

pub struct Mock {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub max_in_flight: Arc<AtomicUsize>,
}

impl Mock {
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    pub fn peak(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }
}

/// The user message of a request body
pub fn user_message(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    v.pointer("/messages/1/content")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string()
}

/// A chat reply carrying `content`
pub fn reply(content: &str) -> Answer {
    (
        200,
        serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]})
            .to_string(),
    )
}

/// The `N| text` lines to translate in a user message (after the context block)
pub fn lines_to_translate(user: &str) -> Vec<(usize, String)> {
    let body = user
        .rsplit("Translate these lines:\n")
        .next()
        .unwrap_or(user);
    body.lines()
        .filter_map(|l| {
            let (n, t) = l.split_once("| ")?;
            Some((n.trim().parse().ok()?, t.to_string()))
        })
        .collect()
}

/// Translate every line by prefixing its text
pub fn echo(prefix: &'static str) -> impl Fn(&str, usize) -> Answer + Send + Sync + 'static {
    move |user, _| {
        let out: Vec<String> = lines_to_translate(user)
            .iter()
            .map(|(n, t)| format!("{n}| {prefix}{t}"))
            .collect();
        reply(&out.join("\n"))
    }
}

pub async fn serve(
    delay: Duration,
    handler: impl Fn(&str, usize) -> Answer + Send + Sync + 'static,
) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let handler: Arc<Handler> = Arc::new(handler);
    let (r, f, p) = (requests.clone(), in_flight, peak.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let (handler, requests, in_flight, peak) =
                (handler.clone(), r.clone(), f.clone(), p.clone());
            tokio::spawn(async move {
                loop {
                    let Some(body) = read_request(&mut sock).await else {
                        return;
                    };
                    let user = user_message(&body);
                    let index = {
                        let mut all = requests.lock().unwrap();
                        all.push(user.clone());
                        all.len() - 1
                    };
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    let (status, out) = handler(&user, index);
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{out}",
                        out.len()
                    );
                    if sock.write_all(resp.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Mock {
        url,
        requests,
        max_in_flight: peak,
    }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            while buf.len() < end + 4 + len {
                let n = sock.read(&mut chunk).await.ok()?;
                if n == 0 {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            return Some(String::from_utf8_lossy(&buf[end + 4..end + 4 + len]).into_owned());
        }
    }
}
