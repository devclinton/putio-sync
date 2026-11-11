//! Email against a small fake SMTP server on localhost: plain sending, grouping through the
//! event bus, and STARTTLS that must not fall back to plain text.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use putio_sync_core::email::{SmtpConfig, SmtpSecurity};
use putio_sync_core::events::{self, EventType, Payload};
use putio_sync_notify::{smtp, EmailService};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What the fake server saw
#[derive(Default)]
struct Seen {
    commands: Vec<String>,
    messages: Vec<String>,
}

/// A server that takes every message and offers no STARTTLS. Returns its port.
async fn server() -> (u16, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let shared = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let seen = shared.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"220 fake ESMTP\r\n").await.unwrap();
                let mut data: Option<String> = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(body) = data.as_mut() {
                        if line == "." {
                            seen.lock().unwrap().messages.push(data.take().unwrap());
                            write.write_all(b"250 queued\r\n").await.unwrap();
                        } else {
                            body.push_str(&line);
                            body.push('\n');
                        }
                        continue;
                    }
                    seen.lock().unwrap().commands.push(line.clone());
                    let verb = line.split(' ').next().unwrap_or("").to_ascii_uppercase();
                    let reply: &[u8] = match verb.as_str() {
                        "EHLO" => b"250-fake\r\n250 8BITMIME\r\n",
                        "DATA" => {
                            data = Some(String::new());
                            b"354 go on\r\n"
                        }
                        "QUIT" => {
                            let _ = write.write_all(b"221 bye\r\n").await;
                            return;
                        }
                        _ => b"250 ok\r\n",
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (port, seen)
}

fn config(port: u16, security: SmtpSecurity) -> SmtpConfig {
    SmtpConfig {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: Some(port),
        security,
        from: "putio-sync <nas@example.com>".to_string(),
        to: vec!["me@example.com".to_string(), "you@example.com".to_string()],
        ..SmtpConfig::default()
    }
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn finished(name: &str) -> Payload {
    let mut payload = events::samples()
        .into_iter()
        .find(|p| p.event_type() == EventType::DownloadFinished)
        .unwrap();
    if let Payload::DownloadFinished(d) = &mut payload {
        d.name = name.to_string();
    }
    payload
}

#[tokio::test]
async fn a_plain_message_reaches_every_recipient() {
    let (port, seen) = server().await;
    smtp::send(
        &config(port, SmtpSecurity::None),
        "Downloaded",
        "episode.mkv (1.4 GB)\n",
    )
    .await
    .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.messages.len(), 1);
    let message = &seen.messages[0];
    assert!(
        message.contains("Subject: putio-sync: Downloaded"),
        "{message}"
    );
    assert!(message.contains("episode.mkv (1.4 GB)"), "{message}");
    assert!(message.contains("text/plain"), "{message}");
    let rcpt: Vec<_> = seen
        .commands
        .iter()
        .filter(|c| c.starts_with("RCPT"))
        .collect();
    assert_eq!(rcpt.len(), 2, "{:?}", seen.commands);
    // No login is sent without encryption
    assert!(!seen.commands.iter().any(|c| c.starts_with("AUTH")));
}

#[tokio::test]
async fn starttls_that_the_server_lacks_fails_without_sending() {
    let (port, seen) = server().await;
    let mut c = config(port, SmtpSecurity::Starttls);
    c.username = "me".to_string();
    c.password = "s3cret-smtp".to_string();
    let error = smtp::send(&c, "Downloaded", "x\n").await.unwrap_err();
    assert!(error.contains(&format!("127.0.0.1:{port}")), "{error}");
    assert!(!error.contains("s3cret"), "{error}");
    let seen = seen.lock().unwrap();
    assert!(seen.messages.is_empty());
    assert!(
        !seen
            .commands
            .iter()
            .any(|c| c.starts_with("AUTH") || c.starts_with("MAIL")),
        "{:?}",
        seen.commands
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bus_events_are_sent_and_repeats_grouped() {
    let _turn = SERIAL.lock().await;
    let (port, seen) = server().await;
    let email = EmailService::new(config(port, SmtpSecurity::None));
    email.start();
    for i in 0..3 {
        events::emit(finished(&format!("File {i}")));
    }
    // Not chosen: sent nothing
    let added = events::samples()
        .into_iter()
        .find(|p| p.event_type() == EventType::TransferAdded)
        .unwrap();
    events::emit(added);
    wait_for("the first email", || email.status().sent >= 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status = email.status();
    assert_eq!((status.sent, status.failed), (1, 0));
    assert_eq!(status.last.unwrap().subject, "Downloaded");
    let message = seen.lock().unwrap().messages[0].clone();
    assert!(message.contains("File 0"), "{message}");
    assert!(message.contains("Sent by putio-sync on "), "{message}");

    // Turned off: nothing more is sent
    email.configure(SmtpConfig {
        enabled: false,
        group_minutes: 0,
        ..config(port, SmtpSecurity::None)
    });
    events::emit(finished("File 9"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(email.status().sent, 1);
    assert!(!email.status().enabled);
}

#[tokio::test]
async fn the_test_email_reports_failure() {
    // Nothing listens on this port once the listener is dropped
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let email = EmailService::new(SmtpConfig::default());
    let error = email
        .test(&config(port, SmtpSecurity::None))
        .await
        .unwrap_err();
    assert!(error.contains(&format!("127.0.0.1:{port}")), "{error}");
    let status = email.status();
    assert_eq!(status.failed, 1);
    assert_eq!(status.last.unwrap().error, Some(error));
}
