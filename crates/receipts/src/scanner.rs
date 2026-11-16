//! The malware scanner: a port, and a clamd client for it.
//!
//! Java counterpart: `receipts/internal/application/MalwareScanner.java` and
//! `internal/infrastructure/ClamAvScanner.java`. The client speaks clamd's `INSTREAM` protocol: `zINSTREAM\0`,
//! then the file as chunks of a 4-byte big-endian length and the bytes, then a zero-length chunk; clamd answers
//! `stream: OK` or `stream: <signature> FOUND`, ended by a NUL byte.
//!
//! The retries Java declares on the client (`@Retryable`) are in [`crate::application::ScanReceipt`], which
//! opens the object again for each attempt; Java hands the same, partly read stream to every retry.

use ledgerly_shared::observation::Observation;
use std::fmt;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

use ledgerly_shared::config::ClamavConfig;
use ledgerly_shared::storage::ObjectReader;
use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::Instrument;

/// Java's `ClamAvScanner.CHUNK`.
const CHUNK: usize = 8192;
/// clamd's answers are a line; anything longer is not one of them.
const MAX_REPLY: usize = 1024;

/// The verdict on a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// No malware found.
    Clean,
    /// Malware found; the scanner's name for it.
    Infected { signature: String },
}

/// The scanner could not be reached, or did not clearly answer. The caller may try again.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ScanError {
    #[error("the scanner cannot be reached: {0}")]
    Unreachable(#[source] std::io::Error),
    #[error("the scanner did not answer in time")]
    TimedOut,
    #[error("the file could not be read: {0}")]
    Content(#[source] std::io::Error),
    #[error("Unexpected ClamAV reply")]
    UnexpectedReply,
}

impl ScanError {
    /// For the log: the kind of failure, as Java logs the exception's class name.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "unreachable",
            Self::TimedOut => "timed out",
            Self::Content(_) => "content unreadable",
            Self::UnexpectedReply => "unexpected reply",
        }
    }
}

pub(crate) type ScanFuture<'a> = Pin<Box<dyn Future<Output = Result<Verdict, ScanError>> + Send + 'a>>;

/// Port: checks file content for malware.
pub(crate) trait MalwareScanner: fmt::Debug + Send + Sync {
    /// Reads `content` to its end and says what it is.
    fn scan<'a>(&'a self, content: &'a mut ObjectReader) -> ScanFuture<'a>;
}

/// The clamd client. One connection per scan.
#[derive(Debug, Clone)]
pub(crate) struct ClamAvScanner {
    config: ClamavConfig,
}

impl ClamAvScanner {
    pub(crate) const fn new(config: ClamavConfig) -> Self {
        Self { config }
    }

    async fn scan_once(&self, content: &mut ObjectReader) -> Result<Verdict, ScanError> {
        let address = (self.config.host.as_str(), self.config.port);
        let mut socket = tokio::time::timeout(self.config.connect_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| ScanError::TimedOut)?
            .map_err(ScanError::Unreachable)?;
        let timeout = self.config.read_timeout;
        within(timeout, socket.write_all(b"zINSTREAM\0")).await?;
        let mut buffer = vec![0_u8; CHUNK];
        loop {
            let read = content.read(&mut buffer).await.map_err(ScanError::Content)?;
            if read == 0 {
                break;
            }
            let length = u32::try_from(read).map_err(|_| ScanError::UnexpectedReply)?;
            within(timeout, socket.write_all(&length.to_be_bytes())).await?;
            within(timeout, socket.write_all(buffer.get(..read).unwrap_or_default())).await?;
        }
        within(timeout, socket.write_all(&[0, 0, 0, 0])).await?;
        within(timeout, socket.flush()).await?;
        let reply = read_reply(&mut socket, timeout).await?;
        parse(&reply)
    }
}

impl MalwareScanner for ClamAvScanner {
    /// Each attempt is a `ledgerly.clamav.scan` observation (timer and span `clamav scan`) and log span with its
    /// outcome (`clean`, `infected` or `error`), never anything of the content. Java: `ClamAvScanner.scan`.
    fn scan<'a>(&'a self, content: &'a mut ObjectReader) -> ScanFuture<'a> {
        let span = tracing::info_span!("ledgerly.clamav.scan", outcome = tracing::field::Empty);
        Box::pin(
            async move {
                let mut observation = Observation::start("ledgerly.clamav.scan", "clamav scan");
                let result = self.scan_once(content).await;
                let outcome = match &result {
                    Ok(Verdict::Clean) => "clean",
                    Ok(Verdict::Infected { .. }) => "infected",
                    Err(_) => "error",
                };
                observation.tag("outcome", outcome);
                if result.is_err() {
                    // Java's `IOException`, the only failure `scanOnce` throws.
                    observation.error("IOException");
                }
                observation.stop();
                tracing::Span::current().record("outcome", outcome);
                result
            }
            .instrument(span),
        )
    }
}

/// An I/O step that gives up after `timeout`. Java's socket has a read timeout only; a write that never
/// completes is cut off here too.
async fn within<T>(timeout: Duration, step: impl Future<Output = std::io::Result<T>>) -> Result<T, ScanError> {
    tokio::time::timeout(timeout, step)
        .await
        .map_err(|_| ScanError::TimedOut)?
        .map_err(ScanError::Unreachable)
}

/// clamd's NUL-terminated answer, as ASCII text, stripped. Java reads until a NUL byte or the end of the
/// stream; each read may wait `timeout`, as Java's socket timeout does.
async fn read_reply(socket: &mut TcpStream, timeout: Duration) -> Result<String, ScanError> {
    let mut reply = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = within(timeout, socket.read(&mut byte)).await?;
        if read == 0 || byte[0] == 0 {
            break;
        }
        if reply.len() == MAX_REPLY {
            return Err(ScanError::UnexpectedReply);
        }
        reply.push(byte[0]);
    }
    // Java decodes as US-ASCII, which turns every other byte into U+FFFD.
    let text: String = reply
        .into_iter()
        .map(|b| if b.is_ascii() { char::from(b) } else { '\u{FFFD}' })
        .collect();
    Ok(ledgerly_shared::text::strip(&text).to_owned())
}

static FOUND: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^stream: ([A-Za-z0-9._+-]{1,200}) FOUND$").ok());

/// The reply comes from another system, so it is matched against the exact shapes clamd uses and nothing
/// else. Anything else (an error message, a reply that mentions both OK and FOUND, garbage) is an error, never
/// "clean": a file is accepted only when the scanner clearly said so.
pub(crate) fn parse(reply: &str) -> Result<Verdict, ScanError> {
    if reply == "stream: OK" {
        return Ok(Verdict::Clean);
    }
    FOUND
        .as_ref()
        .and_then(|found| found.captures(reply))
        .and_then(|captures| captures.get(1))
        .map(|signature| Verdict::Infected {
            signature: signature.as_str().to_owned(),
        })
        .ok_or(ScanError::UnexpectedReply)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tokio::net::TcpListener;

    use super::*;

    /// What the stand-in for clamd received.
    struct Received {
        command: Vec<u8>,
        payload: Vec<u8>,
    }

    /// Accepts one connection, reads a full `INSTREAM` request, answers `reply` and a NUL. Java:
    /// `ClamAvScannerTest.fakeClamd`.
    async fn fake_clamd(reply: &'static str) -> (u16, tokio::task::JoinHandle<Received>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut command = Vec::new();
            loop {
                let b = socket.read_u8().await.unwrap();
                if b == 0 {
                    break;
                }
                command.push(b);
            }
            let mut payload = Vec::new();
            loop {
                let length = socket.read_u32().await.unwrap() as usize;
                if length == 0 {
                    break;
                }
                let mut chunk = vec![0; length];
                socket.read_exact(&mut chunk).await.unwrap();
                payload.extend(chunk);
            }
            socket.write_all(format!("{reply}\0").as_bytes()).await.unwrap();
            Received { command, payload }
        });
        (port, task)
    }

    fn scanner_for(port: u16) -> ClamAvScanner {
        ClamAvScanner::new(ClamavConfig {
            host: "127.0.0.1".to_owned(),
            port,
            connect_timeout: Duration::from_secs(2),
            read_timeout: Duration::from_secs(5),
        })
    }

    fn content(bytes: Vec<u8>) -> ObjectReader {
        Box::pin(Cursor::new(bytes))
    }

    /// Java: `sendsTheFileInChunksAndReadsAnOkReply`.
    #[tokio::test]
    async fn sends_the_file_in_chunks_and_reads_an_ok_reply() {
        let file = vec![7_u8; 20_000];
        let (port, clamd) = fake_clamd("stream: OK").await;

        let verdict = scanner_for(port).scan(&mut content(file.clone())).await.unwrap();

        assert_eq!(verdict, Verdict::Clean);
        let received = clamd.await.unwrap();
        assert_eq!(received.command, b"zINSTREAM");
        assert_eq!(received.payload, file);
    }

    /// Java: `parsesAFoundReplyIntoTheSignatureName`.
    #[tokio::test]
    async fn parses_a_found_reply_into_the_signature_name() {
        let (port, _clamd) = fake_clamd("stream: Eicar-Test-Signature FOUND").await;

        let verdict = scanner_for(port).scan(&mut content(vec![1, 2, 3])).await.unwrap();

        assert_eq!(
            verdict,
            Verdict::Infected {
                signature: "Eicar-Test-Signature".to_owned()
            }
        );
    }

    /// Java: `anErrorReplyIsAnIOExceptionSoItCanBeRetried`.
    #[tokio::test]
    async fn an_error_reply_is_an_error_so_it_can_be_retried() {
        let (port, _clamd) = fake_clamd("INSTREAM size limit exceeded. ERROR").await;

        let error = scanner_for(port).scan(&mut content(vec![1])).await.unwrap_err();

        assert_eq!(error.to_string(), "Unexpected ClamAV reply");
    }

    /// Java: `anUnreachableScannerIsAnIOException`.
    #[tokio::test]
    async fn an_unreachable_scanner_is_an_error() {
        let closed = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();

        let error = scanner_for(closed).scan(&mut content(vec![1])).await.unwrap_err();

        assert!(matches!(error, ScanError::Unreachable(_)), "{error}");
    }

    #[tokio::test]
    async fn a_scanner_that_never_answers_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _silent = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
            drop(socket);
        });
        let scanner = ClamAvScanner::new(ClamavConfig {
            host: "127.0.0.1".to_owned(),
            port,
            connect_timeout: Duration::from_secs(2),
            read_timeout: Duration::from_millis(200),
        });

        let error = scanner.scan(&mut content(vec![1])).await.unwrap_err();

        assert!(matches!(error, ScanError::TimedOut), "{error}");
    }

    /// Java: `anythingButTheTwoExactRepliesIsAnErrorNeverClean`.
    #[test]
    fn anything_but_the_two_exact_replies_is_an_error_never_clean() {
        for reply in [
            "",
            "OK",
            "stream: OK extra",
            "stream: Evil OK",
            "stream: Eicar FOUND OK",
            "stream: FOUND",
            "stream:  FOUND",
            "stream: bad name FOUND",
            "INSTREAM size limit exceeded. ERROR",
            "<html>not clamd</html>",
            "stream: ok",
        ] {
            assert!(parse(reply).is_err(), "{reply:?}");
        }
        let too_long = format!("stream: {} FOUND", "a".repeat(201));
        assert!(parse(&too_long).is_err());
    }

    /// Java: `theTwoExactRepliesAreUnderstood`.
    #[test]
    fn the_two_exact_replies_are_understood() {
        assert_eq!(parse("stream: OK").unwrap(), Verdict::Clean);
        assert_eq!(
            parse("stream: Win.Test.EICAR_HDB-1 FOUND").unwrap(),
            Verdict::Infected {
                signature: "Win.Test.EICAR_HDB-1".to_owned()
            }
        );
    }
}
