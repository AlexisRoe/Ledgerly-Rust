//! The JSON body limit also holds for a body sent in chunks, whose size is not announced up front.
//!
//! Java: `src/test/java/com/ledgerly/shared/web/ChunkedBodyLimitTest.java`. The request is written by hand on
//! a TCP connection, so that it really goes out with `Transfer-Encoding: chunked`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::Target;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// POSTs `json` to the login endpoint in 64 KiB chunks and returns the status and the body.
async fn post_chunked(json: &str) -> (u16, String) {
    let target = Target::from_env();
    let authority = target
        .base_url
        .split_once("://")
        .map_or(target.base_url.as_str(), |(_, rest)| rest);
    let mut stream = TcpStream::connect(authority).await.unwrap();
    let head = format!(
        "POST /api/v1/auth/login HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\n\
         Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    for chunk in json.as_bytes().chunks(64 * 1024) {
        // The server may answer and close before it has read everything; that is the point.
        if stream
            .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
            .await
            .is_err()
            || stream.write_all(chunk).await.is_err()
            || stream.write_all(b"\r\n").await.is_err()
        {
            break;
        }
    }
    let _ = stream.write_all(b"0\r\n\r\n").await;
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response).await;
    let text = String::from_utf8_lossy(&response).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (status, text)
}

mod m8 {
    use super::*;

    /// Java: `ChunkedBodyLimitTest.aChunkedBodyOverTheLimitIsRefusedWith413`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_chunked_body_over_the_limit_is_refused_with_413() {
        let huge = format!(
            r#"{{"email":"a@example.com","password":"{}"}}"#,
            "x".repeat(2 * 1024 * 1024)
        );

        let (status, text) = post_chunked(&huge).await;

        assert_eq!(status, 413);
        assert!(text.contains("PAYLOAD_TOO_LARGE"), "{text}");
    }
}

mod m5 {
    use super::*;

    /// Java: `ChunkedBodyLimitTest.aSmallChunkedBodyIsNotTouchedByTheLimit`: it reaches the login, which
    /// refuses an unknown user.
    #[tokio::test]
    async fn a_small_chunked_body_is_not_touched_by_the_limit() {
        let (status, _) = post_chunked(r#"{"email":"nobody@example.com","password":"correct-horse-battery"}"#).await;

        assert_eq!(status, 401);
    }
}
