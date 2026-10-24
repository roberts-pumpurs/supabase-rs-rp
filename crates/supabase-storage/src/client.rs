use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Method, RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{ApiErrorBody, StorageError};
use crate::types::{BucketInfo, BucketOptions};
use crate::{Bucket, path};

/// Follows up to 10 redirects, but only to the same origin (scheme, host, port), so
/// credential headers never reach another host.
fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let same_origin = attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.origin() == attempt.url().origin());
        if !same_origin {
            attempt.stop()
        } else if attempt.previous().len() > 10 {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

/// Client for the Supabase Storage API at `<project>/storage/v1`.
///
/// Cloning is cheap; clones share the HTTP connection pool.
#[derive(Debug, Clone)]
pub struct StorageClient {
    http: reqwest::Client,
    base: Url,
    headers: HeaderMap,
    retry_policy: rp_supabase_http::RetryPolicy,
}

#[derive(Serialize)]
struct BucketBody<'a> {
    id: &'a str,
    name: &'a str,
    #[serde(flatten)]
    options: &'a BucketOptions,
}

impl StorageClient {
    /// Creates a client for the project at `project_url` (for example `https://abc.supabase.co/`).
    ///
    /// Requests send `apikey: <api_key>`. A legacy JWT key is also sent as
    /// `Authorization: Bearer <api_key>`. A new-format key (`sb_publishable_...` or
    /// `sb_secret_...`) is not a JWT, so the client sends no `Authorization` header until
    /// [`StorageClient::with_access_token`] sets one.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL cannot be a base, `api_key` is not a valid header value, or
    /// the HTTP client cannot be built.
    pub fn new(project_url: &Url, api_key: &str) -> Result<Self, StorageError> {
        let http = rp_supabase_http::client_builder()
            .redirect(same_origin_redirects())
            .build()?;
        Self::new_with_client(project_url, api_key, http)
    }

    /// Like [`StorageClient::new`], but reuses the connection pool of `http`.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL cannot be a base or `api_key` is not a valid header value.
    pub fn new_with_client(
        project_url: &Url,
        api_key: &str,
        http: reqwest::Client,
    ) -> Result<Self, StorageError> {
        let base = storage_base(project_url)?;
        let mut headers = HeaderMap::new();
        headers.insert("apikey", sensitive(api_key)?);
        if !(api_key.starts_with("sb_publishable_") || api_key.starts_with("sb_secret_")) {
            headers.insert(AUTHORIZATION, sensitive(&format!("Bearer {api_key}"))?);
        }
        Ok(Self {
            http,
            base,
            headers,
            retry_policy: rp_supabase_http::RetryPolicy::default(),
        })
    }

    /// Returns a copy that sends `Authorization: Bearer <token>` (a user JWT) so that row level
    /// security policies apply to that user. The `apikey` header stays unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when `token` is not a valid header value.
    pub fn with_access_token(&self, token: &str) -> Result<Self, StorageError> {
        let mut next = self.clone();
        next.headers
            .insert(AUTHORIZATION, sensitive(&format!("Bearer {token}"))?);
        Ok(next)
    }

    /// Returns a copy that sends requests to the project at `project_url` (joined with
    /// `storage/v1`). All headers, including any access token, stay unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL cannot be a base.
    pub fn with_project_url(&self, project_url: &Url) -> Result<Self, StorageError> {
        Ok(Self {
            http: self.http.clone(),
            base: storage_base(project_url)?,
            headers: self.headers.clone(),
            retry_policy: self.retry_policy,
        })
    }

    /// Returns a handle for object operations in `bucket`. No request is sent.
    #[must_use]
    pub const fn from<'a>(&'a self, bucket: &'a str) -> Bucket<'a> {
        Bucket::new(self, bucket)
    }

    /// Sets the policy for explicit rate-limit responses.
    #[must_use]
    pub const fn with_retry_policy(mut self, policy: rp_supabase_http::RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Lists all buckets visible to the current credentials.
    ///
    /// # Errors
    ///
    /// Returns an error on transport failure, non-success status, or an unexpected body.
    pub async fn list_buckets(&self) -> Result<Vec<BucketInfo>, StorageError> {
        self.json(self.request(Method::GET, self.url(["bucket"])))
            .await
    }

    /// Fetches one bucket.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid id, transport failure, non-success status, or an unexpected body.
    pub async fn get_bucket(&self, id: &str) -> Result<BucketInfo, StorageError> {
        let url = self.url(["bucket", path::bucket(id)?]);
        self.json(self.request(Method::GET, url)).await
    }

    /// Creates a bucket whose id and name are `id`.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid id, transport failure, or non-success status.
    pub async fn create_bucket(
        &self,
        id: &str,
        options: &BucketOptions,
    ) -> Result<(), StorageError> {
        let body = BucketBody {
            id: path::bucket(id)?,
            name: id,
            options,
        };
        self.send(self.request(Method::POST, self.url(["bucket"])).json(&body))
            .await?;
        Ok(())
    }

    /// Replaces the settings of a bucket.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid id, transport failure, or non-success status.
    pub async fn update_bucket(
        &self,
        id: &str,
        options: &BucketOptions,
    ) -> Result<(), StorageError> {
        let url = self.url(["bucket", path::bucket(id)?]);
        let body = BucketBody {
            id,
            name: id,
            options,
        };
        self.send(self.request(Method::PUT, url).json(&body))
            .await?;
        Ok(())
    }

    /// Deletes all objects in a bucket and keeps the bucket.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid id, transport failure, or non-success status.
    pub async fn empty_bucket(&self, id: &str) -> Result<(), StorageError> {
        let url = self.url(["bucket", path::bucket(id)?, "empty"]);
        self.send(
            self.request(Method::POST, url)
                .json(&serde_json::Map::new()),
        )
        .await?;
        Ok(())
    }

    /// Deletes a bucket. The API rejects this when the bucket is not empty; call
    /// [`StorageClient::empty_bucket`] first.
    ///
    /// # Errors
    ///
    /// Returns an error on an invalid id, transport failure, or non-success status.
    pub async fn delete_bucket(&self, id: &str) -> Result<(), StorageError> {
        let url = self.url(["bucket", path::bucket(id)?]);
        self.send(self.request(Method::DELETE, url)).await?;
        Ok(())
    }

    /// Builds `<base>/<segments...>`; each segment is percent-encoded once.
    pub(crate) fn url<'a, I: IntoIterator<Item = &'a str>>(&self, segments: I) -> Url {
        let mut url = self.base.clone();
        // The constructor rejects cannot-be-a-base URLs, so this always succeeds.
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    pub(crate) fn request(&self, method: Method, url: Url) -> RequestBuilder {
        self.http.request(method, url).headers(self.headers.clone())
    }

    pub(crate) async fn send(&self, request: RequestBuilder) -> Result<Response, StorageError> {
        let (http, request) = request.build_split();
        let response = rp_supabase_http::execute(&http, request?, self.retry_policy).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = ApiErrorBody::decode(response.text().await?);
        Err(StorageError::Api { status, body })
    }

    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T, StorageError> {
        let bytes = self.send(request).await?.bytes().await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

fn storage_base(project_url: &Url) -> Result<Url, StorageError> {
    let base = project_url.join("storage/v1")?;
    if base.cannot_be_a_base() {
        return Err(StorageError::InvalidBaseUrl);
    }
    Ok(base)
}

fn sensitive(value: &str) -> Result<HeaderValue, StorageError> {
    let mut header = HeaderValue::from_str(value)?;
    header.set_sensitive(true);
    Ok(header)
}
