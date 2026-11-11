//! Sending one plain-text email with lettre over rustls.

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use putio_sync_core::email::{SmtpConfig, SmtpSecurity};

/// How long one send may take, connecting included
pub const TIMEOUT: Duration = Duration::from_secs(30);

fn mailbox(field: &str, text: &str) -> Result<Mailbox, String> {
    text.trim()
        .parse()
        .map_err(|e| format!("{field} \"{}\": {e}", text.trim()))
}

/// Build the message: `putio-sync: <subject>` from `from` to every `to`
fn build(config: &SmtpConfig, subject: &str, body: &str) -> Result<Message, String> {
    let mut message = Message::builder()
        .from(mailbox("from", &config.from)?)
        .subject(format!("putio-sync: {subject}"))
        .header(ContentType::TEXT_PLAIN);
    for to in &config.to {
        message = message.to(mailbox("to", to)?);
    }
    message.body(body.to_string()).map_err(|e| e.to_string())
}

/// What went wrong, in words, without the login
fn describe(config: &SmtpConfig, e: &lettre::transport::smtp::Error) -> String {
    let server = format!("{}:{}", config.host.trim(), config.port());
    if e.is_timeout() {
        format!("{server}: no answer within {} s", TIMEOUT.as_secs())
    } else if e.is_tls() {
        format!("{server}: TLS failed ({e})")
    } else if e.is_permanent() || e.is_transient() {
        format!("{server} refused: {e}")
    } else {
        format!("{server}: {e}")
    }
}

/// Send one email. STARTTLS must succeed when asked for: the message and the login are
/// never sent over a plain connection unless security is `none`, which allows no login.
pub async fn send(config: &SmtpConfig, subject: &str, body: &str) -> Result<(), String> {
    let message = build(config, subject, body)?;
    let host = config.host.trim();
    let builder = match config.security {
        SmtpSecurity::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
        SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        SmtpSecurity::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            host,
        )),
    }
    .map_err(|e| describe(config, &e))?;
    let mut builder = builder.port(config.port()).timeout(Some(TIMEOUT));
    let user = config.username.trim();
    if !user.is_empty() && config.security != SmtpSecurity::None {
        builder = builder.credentials(Credentials::new(user.to_string(), config.password.clone()));
    }
    builder
        .build()
        .send(message)
        .await
        .map(|_| ())
        .map_err(|e| describe(config, &e))
}
