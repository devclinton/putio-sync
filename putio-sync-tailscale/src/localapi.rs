//! tailscaled's LocalAPI over its unix socket: a small HTTP/1.0 GET, so no HTTP client
//! with unix-socket support is needed.

use std::path::Path;

/// The most a LocalAPI answer may be
const MAX_RESPONSE: usize = 1024 * 1024;

/// A LocalAPI answer: the status code and the body
#[derive(Debug)]
pub struct Response {
    pub code: u16,
    pub body: Vec<u8>,
}

/// `GET <path>` on the socket. Errors are short, user-facing reasons.
#[cfg(unix)]
pub async fn get(socket: &Path, path: &str) -> Result<Response, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|e| format!("can't open {}: {e}", socket.display()))?;
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: local-tailscaled.sock\r\nSec-Tailscale: localapi\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("can't write to tailscaled: {e}"))?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| format!("can't read from tailscaled: {e}"))?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > MAX_RESPONSE {
            return Err("tailscaled's answer is too large".to_string());
        }
    }
    parse_response(&raw)
}

#[cfg(not(unix))]
pub async fn get(_socket: &Path, _path: &str) -> Result<Response, String> {
    Err("the tailscaled socket needs a unix system".to_string())
}

/// Split an HTTP/1.x response into its code and body, de-chunking when needed
pub(crate) fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "tailscaled's answer isn't HTTP".to_string())?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "bad HTTP head".to_string())?;
    let body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let code = lines
        .next()
        .filter(|l| l.starts_with("HTTP/"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| "tailscaled's answer isn't HTTP".to_string())?;
    let chunked = lines.any(|l| {
        l.split_once(':').is_some_and(|(k, v)| {
            k.trim().eq_ignore_ascii_case("transfer-encoding")
                && v.trim().eq_ignore_ascii_case("chunked")
        })
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    Ok(Response { code, body })
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let bad = || "bad chunked answer from tailscaled".to_string();
    let mut out = Vec::new();
    loop {
        let eol = data.windows(2).position(|w| w == b"\r\n").ok_or_else(bad)?;
        let size_line = std::str::from_utf8(&data[..eol]).map_err(|_| bad())?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| bad())?;
        data = &data[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size {
            return Err(bad());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size..];
        data = data.strip_prefix(b"\r\n").unwrap_or(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_chunked_answers() {
        let r = parse_response(b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}").unwrap();
        assert_eq!(r.code, 200);
        assert_eq!(r.body, b"{\"a\":1}");

        let r = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3;x=y\r\n:1}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.body, b"{\"a\":1}");

        let r = parse_response(b"HTTP/1.0 404 Not Found\r\n\r\nno such peer").unwrap();
        assert_eq!(r.code, 404);

        assert!(parse_response(b"junk").is_err());
        assert!(parse_response(b"SSH-2.0\r\n\r\n").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
    }
}
