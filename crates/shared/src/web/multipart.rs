//! Reading the file of a `multipart/form-data` upload, as it arrives.
//!
//! Java counterpart: Spring's `StandardServletMultipartResolver` on Tomcat, with Java's
//! `spring.servlet.multipart.max-file-size: 10MB` and `max-request-size: 11MB`, and `ProblemExceptionHandler`'s
//! answers:
//! - a part, or the request, over its limit: 413 `PAYLOAD_TOO_LARGE`, "The upload is larger than the limit";
//! - no file part of that name: Spring's own 400, "Required part 'file' is not present.";
//! - a body that is not valid multipart: 400 `MALFORMED_REQUEST`. Java answers 500 `INTERNAL_ERROR` and logs
//!   an error, because its handlers do not map Spring's `MultipartException`; answering a client's mistake as
//!   one is the reviewer's decision after M7 (learning log, M7 open question 2).
//!
//! Spring parses the whole request before the controller runs. Here nothing is read until the caller asks for
//! the file, which the receipts module does only once it knows the caller may upload (plan M7.2).

use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use multer::{Constraints, SizeLimit};

use super::problem::Problem;

/// `spring.servlet.multipart.max-file-size`: the largest part, in bytes (Spring's `10MB` is 10 MiB).
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
/// `spring.servlet.multipart.max-request-size`: the largest request, in bytes.
pub const MAX_REQUEST_SIZE: u64 = 11 * 1024 * 1024;

/// An upload whose parts have not been read yet.
pub struct MultipartUpload {
    declared_length: Option<u64>,
    parts: multer::Multipart<'static>,
}

impl std::fmt::Debug for MultipartUpload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultipartUpload")
            .field("declared_length", &self.declared_length)
            .finish_non_exhaustive()
    }
}

impl MultipartUpload {
    /// Takes the request's body. The route table has checked that the request is `multipart/form-data`.
    ///
    /// # Errors
    ///
    /// The `Content-Type` has no boundary: 400 `MALFORMED_REQUEST`.
    pub fn new(request: Request) -> Result<Self, Problem> {
        let headers = request.headers();
        let boundary = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| multer::parse_boundary(value).ok())
            .ok_or_else(unparsable)?;
        let declared_length = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let constraints =
            Constraints::new().size_limit(SizeLimit::new().whole_stream(MAX_REQUEST_SIZE).per_field(MAX_FILE_SIZE));
        let stream = request.into_body().into_data_stream();
        Ok(Self {
            declared_length,
            parts: multer::Multipart::with_constraints(stream, boundary, constraints),
        })
    }

    /// The first part called `name` that is a file (has a `filename`), as Spring's `getFile(name)` finds it.
    /// The parts before it are read and dropped.
    ///
    /// # Errors
    ///
    /// The request declares more than [`MAX_REQUEST_SIZE`] (Tomcat refuses it before reading), a part is too
    /// large, there is no such part, or the body is not multipart.
    pub async fn file(mut self, name: &str) -> Result<UploadedFile, Problem> {
        if self.declared_length.is_some_and(|length| length > MAX_REQUEST_SIZE) {
            return Err(too_large());
        }
        while let Some(field) = self.parts.next_field().await.map_err(failure)? {
            if field.name() == Some(name)
                && let Some(original_name) = field.file_name().map(str::to_owned)
            {
                return Ok(UploadedFile { field, original_name });
            }
        }
        Err(Problem::rejected(
            StatusCode::BAD_REQUEST,
            format!("Required part '{name}' is not present."),
        ))
    }
}

/// The file part of an upload, read chunk by chunk.
pub struct UploadedFile {
    field: multer::Field<'static>,
    original_name: String,
}

impl std::fmt::Debug for UploadedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The name is the client's and may be personal.
        f.debug_struct("UploadedFile").finish_non_exhaustive()
    }
}

impl UploadedFile {
    /// The `filename` the client gave, as sent: metadata only, never a path.
    #[must_use]
    pub fn original_name(&self) -> &str {
        &self.original_name
    }

    /// The next piece of the content, or `None` at its end.
    ///
    /// # Errors
    ///
    /// The part or the request grows past its limit (413), or the body breaks off or is not multipart (400).
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, Problem> {
        self.field.chunk().await.map_err(failure)
    }
}

fn too_large() -> Problem {
    Problem::payload_too_large("The upload is larger than the limit")
}

/// A body that cannot be parsed as multipart. Java: a 500 from its catch-all (see the module comment).
fn unparsable() -> Problem {
    Problem::malformed_request()
}

fn failure(error: multer::Error) -> Problem {
    match error {
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => too_large(),
        _ => unparsable(),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;

    use super::*;

    fn upload(body: Vec<u8>) -> MultipartUpload {
        let request = Request::builder()
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=XyZ")
            .body(Body::from(body))
            .unwrap();
        MultipartUpload::new(request).unwrap()
    }

    fn part(name: &str, filename: Option<&str>, content: &[u8]) -> Vec<u8> {
        let filename = filename.map(|f| format!("; filename=\"{f}\"")).unwrap_or_default();
        let mut bytes = format!(
            "--XyZ\r\nContent-Disposition: form-data; name=\"{name}\"{filename}\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .into_bytes();
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"\r\n");
        bytes
    }

    fn body(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = parts.concat();
        bytes.extend_from_slice(b"--XyZ--\r\n");
        bytes
    }

    async fn content(mut file: UploadedFile) -> Result<Vec<u8>, Problem> {
        let mut content = Vec::new();
        while let Some(chunk) = file.chunk().await? {
            content.extend_from_slice(&chunk);
        }
        Ok(content)
    }

    #[tokio::test]
    async fn the_named_file_part_is_found_after_other_parts() {
        let upload = upload(body(&[
            part("note", None, b"hello"),
            part("file", None, b"a field, not a file"),
            part("file", Some("lunch.pdf"), b"%PDF-1.4"),
        ]));

        let file = upload.file("file").await.unwrap();

        assert_eq!(file.original_name(), "lunch.pdf");
        assert_eq!(content(file).await.unwrap(), b"%PDF-1.4");
    }

    #[tokio::test]
    async fn an_empty_file_name_is_still_a_file() {
        let file = upload(body(&[part("file", Some(""), b"x")]))
            .file("file")
            .await
            .unwrap();

        assert_eq!(file.original_name(), "");
    }

    #[tokio::test]
    async fn without_the_part_it_is_springs_400() {
        let problem = upload(body(&[part("other", Some("x.txt"), b"x")]))
            .file("file")
            .await
            .unwrap_err();

        assert_eq!(problem.status(), StatusCode::BAD_REQUEST);
        assert_eq!(problem.code(), super::super::ErrorCode::RequestRejected);
        assert_eq!(problem.detail(), Some("Required part 'file' is not present."));
    }

    #[tokio::test]
    async fn ten_mebibytes_pass_and_one_byte_more_is_413() {
        let exactly = vec![0_u8; 10 * 1024 * 1024];
        let file = upload(body(&[part("file", Some("a.pdf"), &exactly)]))
            .file("file")
            .await
            .unwrap();
        assert_eq!(content(file).await.unwrap().len(), exactly.len());

        let over = vec![0_u8; 10 * 1024 * 1024 + 1];
        let file = upload(body(&[part("file", Some("a.pdf"), &over)]))
            .file("file")
            .await
            .unwrap();
        let problem = content(file).await.unwrap_err();
        assert_eq!(problem.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(problem.detail(), Some("The upload is larger than the limit"));
    }

    #[tokio::test]
    async fn a_request_declared_over_eleven_mebibytes_is_413_unread() {
        let request = Request::builder()
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=XyZ")
            .header(header::CONTENT_LENGTH, (11 * 1024 * 1024 + 1).to_string())
            .body(Body::from("--XyZ--\r\n"))
            .unwrap();

        let problem = MultipartUpload::new(request).unwrap().file("file").await.unwrap_err();

        assert_eq!(problem.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_body_that_is_not_multipart_is_a_malformed_request() {
        let problem = upload(b"this is not multipart".to_vec())
            .file("file")
            .await
            .unwrap_err();
        assert_eq!(problem.status(), StatusCode::BAD_REQUEST);
        assert_eq!(problem.code(), super::super::ErrorCode::MalformedRequest);

        let without_boundary = Request::builder()
            .header(header::CONTENT_TYPE, "multipart/form-data")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            MultipartUpload::new(without_boundary).unwrap_err().status(),
            StatusCode::BAD_REQUEST
        );
    }
}
