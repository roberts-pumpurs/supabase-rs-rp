use bytes::Bytes;
use reqwest::header::{CACHE_CONTROL, CONTENT_TYPE};
use reqwest::{Body, Method};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::StorageError;
use crate::types::{FileObject, FileOptions, ListOptions, ObjectKey, SignedUrl};
use crate::{StorageClient, path};

/// Object operations in one bucket. Create it with [`StorageClient::from`].
///
/// Object paths use `/` between segments, for example `avatars/user-1.png`. Each segment is
/// percent-encoded once. Empty paths, empty segments, `.` and `..` return
/// [`StorageError::InvalidPath`].
#[derive(Debug, Clone, Copy)]
pub struct Bucket<'a> {
    client: &'a StorageClient,
    id: &'a str,
}

#[derive(Serialize)]
struct ListBody<'a> {
    prefix: &'a str,
    #[serde(flatten)]
    options: &'a ListOptions,
}

#[derive(Serialize)]
struct RemoveBody<'a> {
    prefixes: &'a [&'a str],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferBody<'a> {
    bucket_id: &'a str,
    source_key: &'a str,
    destination_key: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignBody<'a> {
    expires_in: u64,
    paths: &'a [&'a str],
}

#[derive(Deserialize)]
struct SignedEntry {
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "signedURL", default)]
    signed_url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl<'a> Bucket<'a> {
    pub(crate) const fn new(client: &'a StorageClient, id: &'a str) -> Self {
        Self { client, id }
    }

    pub(crate) const fn client(&self) -> &'a StorageClient {
        self.client
    }

    /// Bucket id of this handle.
    #[must_use]
    pub const fn id(&self) -> &'a str {
        self.id
    }

    /// Uploads a new object (`POST /object/<bucket>/<path>`). Set [`FileOptions::upsert`] to
    /// overwrite an existing object.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path or header, transport failure, non-success status, or
    /// an unexpected body.
    pub async fn upload<B: Into<Body>>(
        &self,
        path: &str,
        body: B,
        options: &FileOptions,
    ) -> Result<ObjectKey, StorageError> {
        self.write(Method::POST, path, body.into(), options).await
    }

    /// Replaces an existing object (`PUT /object/<bucket>/<path>`).
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path or header, transport failure, non-success status, or
    /// an unexpected body.
    pub async fn update<B: Into<Body>>(
        &self,
        path: &str,
        body: B,
        options: &FileOptions,
    ) -> Result<ObjectKey, StorageError> {
        self.write(Method::PUT, path, body.into(), options).await
    }

    async fn write(
        &self,
        method: Method,
        path: &str,
        body: Body,
        options: &FileOptions,
    ) -> Result<ObjectKey, StorageError> {
        let url = self.object_url(&["object"], path)?;
        let upsert = if options.upsert { "true" } else { "false" };
        let mut request = self
            .client
            .request(method, url)
            .header("x-upsert", upsert)
            .body(body);
        if let Some(content_type) = &options.content_type {
            request = request.header(CONTENT_TYPE, content_type.as_str());
        }
        request = request.header(CACHE_CONTROL, format!("max-age={}", options.cache_control));
        self.client.json(request).await
    }

    /// Downloads an object into memory. Use [`Bucket::download_stream`] for large objects or
    /// to enforce a size limit while reading.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, or non-success status.
    pub async fn download(&self, path: &str) -> Result<Bytes, StorageError> {
        Ok(self.download_stream(path).await?.response.bytes().await?)
    }

    /// Starts a download and returns after the response headers arrive. Read the body in
    /// chunks with [`ObjectDownload::chunk`] or [`ObjectDownload::into_stream`].
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, or non-success status. A missing
    /// object is an error here, before any body is read.
    pub async fn download_stream(&self, path: &str) -> Result<ObjectDownload, StorageError> {
        let url = self.object_url(&["object"], path)?;
        let response = self
            .client
            .send(self.client.request(Method::GET, url))
            .await?;
        Ok(ObjectDownload { response })
    }

    /// Lists objects and folders directly under `prefix` (use `""` for the bucket root).
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid bucket id, transport failure, non-success status, or an
    /// unexpected body.
    pub async fn list(
        &self,
        prefix: &str,
        options: &ListOptions,
    ) -> Result<Vec<FileObject>, StorageError> {
        let url = self.client.url(["object", "list", path::bucket(self.id)?]);
        let body = ListBody { prefix, options };
        self.client
            .json(self.client.request(Method::POST, url).json(&body))
            .await
    }

    /// Deletes objects and returns the deleted entries. The API skips missing paths and paths
    /// that row level security hides, without an error. Use [`Bucket::remove_object`] to tell
    /// these cases apart.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, non-success status, or an
    /// unexpected body.
    pub async fn remove(&self, paths: &[&str]) -> Result<Vec<FileObject>, StorageError> {
        for object in paths {
            path::object(object)?;
        }
        let url = self.client.url(["object", path::bucket(self.id)?]);
        let body = RemoveBody { prefixes: paths };
        self.client
            .json(self.client.request(Method::DELETE, url).json(&body))
            .await
    }

    /// Deletes one object (`DELETE /object/<bucket>/<path>`).
    ///
    /// Unlike [`Bucket::remove`], this fails when nothing is deleted.
    /// [`StorageError::api_status`] is `404 Not Found` for a missing object and `403 Forbidden`
    /// when row level security denies the delete.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, or non-success status.
    pub async fn remove_object(&self, path: &str) -> Result<(), StorageError> {
        let url = self.object_url(&["object"], path)?;
        self.client
            .send(self.client.request(Method::DELETE, url))
            .await?;
        Ok(())
    }

    /// Moves (renames) an object inside this bucket.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, or non-success status.
    pub async fn move_object(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let request = self.transfer("move", from, to)?;
        self.client.send(request).await?;
        Ok(())
    }

    /// Copies an object inside this bucket and returns the key of the copy.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, non-success status, or an
    /// unexpected body.
    pub async fn copy_object(&self, from: &str, to: &str) -> Result<ObjectKey, StorageError> {
        self.client.json(self.transfer("copy", from, to)?).await
    }

    fn transfer(
        &self,
        action: &str,
        from: &str,
        to: &str,
    ) -> Result<reqwest::RequestBuilder, StorageError> {
        path::object(from)?;
        path::object(to)?;
        let body = TransferBody {
            bucket_id: path::bucket(self.id)?,
            source_key: from,
            destination_key: to,
        };
        let url = self.client.url(["object", action]);
        Ok(self.client.request(Method::POST, url).json(&body))
    }

    /// Creates an absolute URL that grants read access to one object for `expires_in` seconds.
    ///
    /// This uses the batch signing endpoint. The single-object endpoint signs a key that still
    /// contains percent escapes when the name has a `?`, so its URL fails with an invalid
    /// signature.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, non-success status, an
    /// unexpected body, or [`StorageError::SignFailed`] when the object cannot be signed.
    pub async fn create_signed_url(
        &self,
        path: &str,
        expires_in: u64,
    ) -> Result<Url, StorageError> {
        let entry = self
            .create_signed_urls(&[path], expires_in)
            .await?
            .into_iter()
            .next();
        match entry {
            Some(SignedUrl { url: Some(url), .. }) => Ok(url),
            Some(SignedUrl { error, .. }) => Err(StorageError::SignFailed {
                path: path.to_owned(),
                message: error.unwrap_or_default(),
            }),
            None => Err(StorageError::SignFailed {
                path: path.to_owned(),
                message: "the response has no entry".to_owned(),
            }),
        }
    }

    /// Creates signed URLs for several objects in one request. Paths that cannot be signed
    /// have `url: None` and an `error`.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid path, transport failure, non-success status, or an
    /// unexpected body.
    pub async fn create_signed_urls(
        &self,
        paths: &[&str],
        expires_in: u64,
    ) -> Result<Vec<SignedUrl>, StorageError> {
        for object in paths {
            path::object(object)?;
        }
        let url = self.client.url(["object", "sign", path::bucket(self.id)?]);
        let body = SignBody { expires_in, paths };
        let entries: Vec<SignedEntry> = self
            .client
            .json(self.client.request(Method::POST, url).json(&body))
            .await?;
        entries
            .into_iter()
            .zip(paths)
            .map(|(entry, requested)| {
                let url = entry
                    .signed_url
                    .map(|signed| self.signed_url(requested, &signed))
                    .transpose()?;
                Ok(SignedUrl {
                    path: entry.path,
                    url,
                    error: entry.error,
                })
            })
            .collect()
    }

    /// Builds `<base>/object/sign/<bucket>/<path>?token=<token>` from the known path. The API
    /// returns the object key unescaped, so a key with `?` or `#` breaks a plain URL parse.
    /// The token follows the last `?token=` in `signed`.
    fn signed_url(&self, path: &str, signed: &str) -> Result<Url, StorageError> {
        let (_, token) = signed
            .rsplit_once("?token=")
            .ok_or_else(|| StorageError::MissingSignedToken(signed.to_owned()))?;
        let mut url = self.object_url(&["object", "sign"], path)?;
        url.set_query(Some(&format!("token={token}")));
        Ok(url)
    }

    /// Returns the public URL of an object. No request is sent and the URL works only when the
    /// bucket is public; use [`Bucket::create_signed_url`] for private buckets.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid bucket id or object path.
    pub fn public_url(&self, path: &str) -> Result<Url, StorageError> {
        self.object_url(&["object", "public"], path)
    }

    pub(crate) fn object_url(&self, prefix: &[&str], path: &str) -> Result<Url, StorageError> {
        let bucket = path::bucket(self.id)?;
        let segments = path::object(path)?;
        Ok(self.client.url(
            prefix
                .iter()
                .copied()
                .chain(core::iter::once(bucket))
                .chain(segments),
        ))
    }
}

/// Body of an object download that is read in chunks. Create it with
/// [`Bucket::download_stream`]. The status is already checked, so only transport errors
/// remain.
#[derive(Debug)]
pub struct ObjectDownload {
    response: reqwest::Response,
}

impl ObjectDownload {
    pub(crate) const fn new(response: reqwest::Response) -> Self {
        Self { response }
    }

    /// Body length from the `Content-Length` header, if the server sent one. Use it to reject
    /// an object that is too large before reading. It is not a limit: count the bytes you read.
    #[must_use]
    pub fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }

    /// Reads the next chunk of the body. Returns `None` at the end.
    ///
    /// # Errors
    ///
    /// Returns an error on transport failure.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, StorageError> {
        Ok(self.response.chunk().await?)
    }

    /// Converts the body into a stream of chunks, for example to forward it as an HTTP body.
    pub fn into_stream(
        self,
    ) -> impl futures_util::Stream<Item = Result<Bytes, StorageError>> + Send + 'static {
        futures_util::TryStreamExt::map_err(self.response.bytes_stream(), StorageError::from)
    }
}
