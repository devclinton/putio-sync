//! Bounded reads of HTTP response bodies, so a hostile or broken endpoint cannot make the
//! daemon buffer gigabytes in memory

use reqwest::Response;

/// The most a small body (an error answer, a subtitle file) may hold: 10 MiB
pub const MAX_SMALL_BODY: usize = 10 * 1024 * 1024;

/// The most an error answer is read for: the text only ends up in a message
pub const MAX_ERROR_BODY: usize = 64 * 1024;

/// Why a bounded read stopped
#[derive(Debug)]
pub enum BodyError {
    /// The body is longer than the limit
    TooLarge,
    /// The connection failed while reading
    Http(reqwest::Error),
}

impl std::fmt::Display for BodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BodyError::TooLarge => write!(f, "response body is larger than the allowed limit"),
            BodyError::Http(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BodyError {}

/// Read the body, failing with `TooLarge` once it passes `max` bytes. A `Content-Length`
/// over the limit fails before anything is read; a body without one (or a lying one) is
/// counted as it arrives.
pub async fn read_capped(mut response: Response, max: usize) -> Result<Vec<u8>, BodyError> {
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        return Err(BodyError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(BodyError::Http)? {
        if body.len() + chunk.len() > max {
            return Err(BodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Read an error answer as text for a message: at most `MAX_ERROR_BODY` bytes, the rest is
/// dropped, and an unreadable body gives `fallback`
pub async fn read_error_text(mut response: Response, fallback: &str) -> String {
    let mut body: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let room = MAX_ERROR_BODY - body.len();
                body.extend_from_slice(&chunk[..chunk.len().min(room)]);
                if body.len() >= MAX_ERROR_BODY {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => return fallback.to_string(),
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(server: &mockito::ServerGuard, path: &str) -> Response {
        reqwest::get(format!("{}{}", server.url(), path))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_body_within_the_limit_is_read_whole() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/a")
            .with_body("hello")
            .create_async()
            .await;
        let body = read_capped(get(&server, "/a").await, 5).await.unwrap();
        assert_eq!(body, b"hello");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/b")
            .with_body("hello!")
            .create_async()
            .await;
        let err = read_capped(get(&server, "/b").await, 5).await.unwrap_err();
        assert!(matches!(err, BodyError::TooLarge));
    }

    #[tokio::test]
    async fn a_chunked_body_over_the_limit_is_refused_without_a_length() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/c")
            .with_chunked_body(|w| {
                w.write_all(b"12345")?;
                w.write_all(b"67890")
            })
            .create_async()
            .await;
        let err = read_capped(get(&server, "/c").await, 8).await.unwrap_err();
        assert!(matches!(err, BodyError::TooLarge));
    }

    #[tokio::test]
    async fn an_error_text_is_cut_at_the_limit() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/d")
            .with_body("x".repeat(MAX_ERROR_BODY + 1000))
            .create_async()
            .await;
        let text = read_error_text(get(&server, "/d").await, "unreadable").await;
        assert_eq!(text.len(), MAX_ERROR_BODY);
    }
}
