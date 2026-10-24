use base64::Engine as _;
use bytes::Bytes;
use reqwest::{Method, Response, StatusCode};
use url::Url;

use crate::{Bucket, FileOptions, StorageClient, StorageError};

/// Supabase's required non-final TUS chunk size: 6 MiB.
pub const TUS_CHUNK_SIZE: usize = 6 * 1024 * 1024;

/// A bounded-memory TUS upload session. Persist [`Self::url`] to resume in another process.
///
/// A dropped or failed write leaves the offset dirty; the next write first consults HEAD.
/// The caller must supply bytes beginning at the offset returned by [`Self::refresh`].
pub struct ResumableUpload<'a> {
    client: &'a StorageClient,
    url: Url,
    offset: u64,
    length: u64,
    dirty: bool,
    upsert: bool,
}

impl core::fmt::Debug for ResumableUpload<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResumableUpload")
            .field("offset", &self.offset)
            .field("length", &self.length)
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

fn validate_url(client: &StorageClient, url: &Url) -> Result<(), StorageError> {
    let base = client.url(["upload", "resumable"]);
    if url.origin() != base.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(StorageError::Protocol("unsafe resumable upload URL"));
    }
    Ok(())
}

fn number(response: &Response, name: &'static str) -> Result<u64, StorageError> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .ok_or(StorageError::Protocol(
            "missing or invalid TUS offset/length",
        ))
}

fn version(response: &Response) -> Result<(), StorageError> {
    if response
        .headers()
        .get("tus-resumable")
        .is_none_or(|value| value != "1.0.0")
    {
        return Err(StorageError::Protocol("missing or unsupported TUS version"));
    }
    Ok(())
}

impl<'a> Bucket<'a> {
    /// Creates an authenticated TUS 1.0 session without buffering object contents.
    ///
    /// # Errors
    /// Returns invalid path/header, transport, protocol, or server errors.
    pub async fn create_resumable_upload(
        &self,
        path: &str,
        length: u64,
        options: &FileOptions,
    ) -> Result<ResumableUpload<'a>, StorageError> {
        crate::path::bucket(self.id())?;
        crate::path::object(path)?;
        let encode = |value: &str| base64::engine::general_purpose::STANDARD.encode(value);
        let metadata = format!(
            "bucketName {},objectName {},contentType {},cacheControl {}",
            encode(self.id()),
            encode(path),
            encode(
                options
                    .content_type
                    .as_deref()
                    .unwrap_or("application/octet-stream")
            ),
            encode(&options.cache_control.to_string())
        );
        let endpoint = self.client().url(["upload", "resumable"]);
        validate_url(self.client(), &endpoint)?;
        let response = self
            .client()
            .send(
                self.client()
                    .request(Method::POST, endpoint.clone())
                    .header("tus-resumable", "1.0.0")
                    .header("upload-length", length)
                    .header("upload-metadata", metadata)
                    .header("x-upsert", if options.upsert { "true" } else { "false" }),
            )
            .await?;
        if response.status() != StatusCode::CREATED {
            return Err(StorageError::Protocol("TUS creation must return 201"));
        }
        version(&response)?;
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or(StorageError::Protocol("TUS creation has no Location"))?;
        let url = endpoint.join(location)?;
        validate_url(self.client(), &url)?;
        let offset = response
            .headers()
            .get("upload-offset")
            .map_or(Ok(0), |_| number(&response, "upload-offset"))?;
        if offset > length {
            return Err(StorageError::Protocol("TUS offset exceeds length"));
        }
        Ok(ResumableUpload {
            client: self.client(),
            url,
            offset,
            length,
            dirty: false,
            upsert: options.upsert,
        })
    }
}

impl StorageClient {
    /// Reopens a persisted upload URL and obtains authoritative offset and length with HEAD.
    /// Pass the same overwrite intent used when creating the upload; persist it with the URL.
    ///
    /// # Errors
    /// Returns unsafe URL, transport, expiration (404/410), or protocol errors.
    pub async fn resume_upload(
        &self,
        url: &Url,
        upsert: bool,
    ) -> Result<ResumableUpload<'_>, StorageError> {
        validate_url(self, url)?;
        let mut upload = ResumableUpload {
            client: self,
            url: url.clone(),
            offset: 0,
            length: 0,
            dirty: true,
            upsert,
        };
        upload.refresh().await?;
        Ok(upload)
    }
}

impl ResumableUpload<'_> {
    /// Resume handle. Treat this URL as sensitive when persisting it.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Last confirmed offset; consult [`Self::refresh`] after a cancelled/failed write.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// Declared object length.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// Whether an attempted write needs server reconciliation.
    #[must_use]
    pub const fn needs_refresh(&self) -> bool {
        self.dirty
    }

    /// Obtains the authoritative server offset and length. Expired sessions return server errors.
    ///
    /// # Errors
    /// Returns transport, server, or malformed TUS header errors.
    pub async fn refresh(&mut self) -> Result<u64, StorageError> {
        self.dirty = true;
        let response = self
            .client
            .send(
                self.client
                    .request(Method::HEAD, self.url.clone())
                    .header("tus-resumable", "1.0.0")
                    .header("x-upsert", if self.upsert { "true" } else { "false" }),
            )
            .await?;
        version(&response)?;
        let offset = number(&response, "upload-offset")?;
        let length = number(&response, "upload-length")?;
        if offset > length {
            return Err(StorageError::Protocol("TUS offset exceeds length"));
        }
        self.offset = offset;
        self.length = length;
        self.dirty = false;
        Ok(offset)
    }

    /// Writes exactly one chunk, 6 MiB except the final tail (including an empty object).
    ///
    /// `offset` identifies the supplied bytes. A dirty session first performs HEAD and rejects
    /// stale bytes rather than replaying them at a different position. PATCH is never retried.
    /// After a conflict, ambiguous failure, or mismatched response, call `refresh` and supply
    /// bytes from its returned position. Cancellation also leaves this session dirty.
    /// Creation completes zero-byte objects. Empty chunks on a confirmed complete upload
    /// return its offset without another PATCH.
    ///
    /// # Errors
    /// Returns stale offset, invalid size, transport, server, or protocol errors.
    pub async fn write_chunk(&mut self, offset: u64, chunk: Bytes) -> Result<u64, StorageError> {
        if self.dirty {
            self.refresh().await?;
        }
        if offset != self.offset {
            return Err(StorageError::Protocol("stale TUS chunk offset"));
        }
        let remaining = self
            .length
            .checked_sub(self.offset)
            .ok_or(StorageError::Protocol("TUS offset exceeds length"))?;
        let chunk_size = u64::try_from(TUS_CHUNK_SIZE)
            .map_err(|_error| StorageError::Protocol("TUS chunk size exceeds u64"))?;
        let expected = remaining.min(chunk_size);
        let chunk_length = u64::try_from(chunk.len())
            .map_err(|_error| StorageError::Protocol("TUS chunk length exceeds u64"))?;
        let expected_offset = offset
            .checked_add(expected)
            .ok_or(StorageError::Protocol("TUS chunk offset overflow"))?;
        if chunk_length != expected {
            return Err(StorageError::Protocol(
                "TUS chunk must be 6 MiB except the final tail",
            ));
        }
        if remaining == 0 {
            return Ok(self.offset);
        }
        self.dirty = true;
        let request = self
            .client
            .request(Method::PATCH, self.url.clone())
            .header("tus-resumable", "1.0.0")
            .header("x-upsert", if self.upsert { "true" } else { "false" })
            .header("upload-offset", offset)
            .header("content-type", "application/offset+octet-stream")
            .body(chunk);
        let (http, request) = request.build_split();
        let response =
            rp_supabase_http::execute(&http, request?, rp_supabase_http::RetryPolicy::none())
                .await?;
        if response.status() == StatusCode::CONFLICT {
            self.refresh().await?;
            return Err(StorageError::Protocol(
                "TUS offset conflict; offset refreshed",
            ));
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = crate::ApiErrorBody::decode(response.text().await?);
            return Err(StorageError::Api { status, body });
        }
        if response.status() != StatusCode::NO_CONTENT {
            return Err(StorageError::Protocol("TUS PATCH must return 204"));
        }
        version(&response)?;
        let confirmed = number(&response, "upload-offset")?;
        if confirmed != expected_offset {
            self.refresh().await?;
            return Err(StorageError::Protocol(
                "TUS PATCH offset mismatch; offset refreshed",
            ));
        }
        self.offset = confirmed;
        self.dirty = false;
        Ok(confirmed)
    }
}
