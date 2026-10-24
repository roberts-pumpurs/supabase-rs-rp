#![cfg_attr(doc, doc = include_str!("../README.md"))]

use core::fmt;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
pub use rp_supabase_http::RetryPolicy;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
pub use url;
use url::Url;

/// Request construction, transport, HTTP, or response decoding failure.
/// GraphQL execution failures instead appear in [`GraphqlResponse::errors`].
#[derive(Debug, thiserror::Error)]
pub enum GraphqlClientError {
    /// The project URL cannot hold a path.
    #[error("project URL cannot be a base")]
    UrlNotBase,
    /// Invalid project URL.
    #[error(transparent)]
    Url(#[from] url::ParseError),
    /// Invalid header name.
    #[error(transparent)]
    HeaderName(#[from] reqwest::header::InvalidHeaderName),
    /// Invalid header value.
    #[error(transparent)]
    HeaderValue(#[from] reqwest::header::InvalidHeaderValue),
    /// Variables or the request could not be serialized.
    #[error("GraphQL request serialization failed: {0}")]
    Serialize(serde_json::Error),
    /// The HTTP request failed.
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    /// Non-success HTTP status; the body is retained for caller inspection.
    #[error("GraphQL HTTP error {status}")]
    Http {
        /// HTTP response status.
        status: reqwest::StatusCode,
        /// Response body.
        body: String,
    },
    /// JSON did not match the response envelope or requested data type.
    #[error("GraphQL response decoding failed: {0}")]
    Decode(serde_json::Error),
    /// Neither data nor a nonempty errors array was returned.
    #[error("GraphQL response contains neither data nor errors")]
    EmptyEnvelope,
}

/// A source position in the GraphQL document (one-based).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphqlLocation {
    /// Source line.
    pub line: u64,
    /// Source column.
    pub column: u64,
}

/// A response field or zero-based list index in an execution error path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GraphqlPathSegment {
    /// Response field name, including aliases.
    Field(String),
    /// Zero-based list index.
    Index(u64),
}

/// A GraphQL execution or validation error, not an HTTP failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphqlError {
    /// Human-readable error message.
    pub message: String,
    /// Source positions, when supplied by the server.
    #[serde(default)]
    pub locations: Vec<GraphqlLocation>,
    /// Response path, when supplied by the server.
    #[serde(default)]
    pub path: Vec<GraphqlPathSegment>,
    /// Implementation-specific error metadata.
    #[serde(default)]
    pub extensions: Option<serde_json::Value>,
}

/// A response that preserves partial data alongside execution errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphqlResponse<T> {
    /// Typed data, absent for null or omitted data.
    pub data: Option<T>,
    /// Execution or validation errors; an empty list means success.
    #[serde(default)]
    pub errors: Vec<GraphqlError>,
    /// Implementation-specific response metadata.
    pub extensions: Option<serde_json::Value>,
}

impl<T> GraphqlResponse<T> {
    /// Reject execution errors while retaining the entire partial response on failure.
    ///
    /// # Errors
    /// Returns this response unchanged if it contains execution errors.
    pub const fn into_result(self) -> Result<Self, Self> {
        if self.errors.is_empty() {
            Ok(self)
        } else {
            Err(self)
        }
    }
}

/// A cloneable client for the project's `graphql/v1` endpoint.
#[derive(Debug, Clone)]
pub struct GraphqlClient {
    http: reqwest::Client,
    endpoint: Url,
    headers: HeaderMap,
    retry_policy: RetryPolicy,
}

impl GraphqlClient {
    /// Create a same-origin-redirect-only HTTP client.
    /// Legacy keys also supply a bearer token; new publishable/secret keys do not.
    ///
    /// # Errors
    /// Returns invalid URL, credential-header, or HTTP client construction errors.
    pub fn new(project_url: &Url, api_key: &str) -> Result<Self, GraphqlClientError> {
        Self::new_with_client(
            project_url,
            api_key,
            rp_supabase_http::client_builder().build()?,
        )
    }

    /// Use an existing HTTP client without changing its TLS or redirect configuration.
    ///
    /// # Errors
    /// Returns invalid URL or credential-header errors.
    pub fn new_with_client(
        project_url: &Url,
        api_key: &str,
        http: reqwest::Client,
    ) -> Result<Self, GraphqlClientError> {
        if project_url.cannot_be_a_base() {
            return Err(GraphqlClientError::UrlNotBase);
        }
        let mut root = project_url.clone();
        if !root.path().ends_with('/') {
            root.set_path(&format!("{}/", root.path()));
        }
        let endpoint = root.join("graphql/v1")?;
        let mut headers = HeaderMap::new();
        headers.insert("apikey", sensitive(api_key)?);
        if !api_key.starts_with("sb_publishable_") && !api_key.starts_with("sb_secret_") {
            headers.insert(AUTHORIZATION, sensitive(&format!("Bearer {api_key}"))?);
        }
        Ok(Self {
            http,
            endpoint,
            headers,
            retry_policy: RetryPolicy::default(),
        })
    }

    /// Clone the client with a user's bearer token, preserving its retry policy.
    ///
    /// # Errors
    /// Returns an error if the token is not a valid header value.
    pub fn with_access_token(&self, token: &str) -> Result<Self, GraphqlClientError> {
        let mut next = self.clone();
        next.headers
            .insert(AUTHORIZATION, sensitive(&format!("Bearer {token}"))?);
        Ok(next)
    }

    /// Configure 429 retries. POST requests are never replayed unless explicitly opted in.
    #[must_use]
    pub const fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Start a POST request. Documents and variables are omitted from builder debug output.
    pub fn query(&self, document: &str) -> QueryBuilder {
        QueryBuilder {
            http: self.http.clone(),
            endpoint: self.endpoint.clone(),
            retry_policy: self.retry_policy,
            headers: self.headers.clone(),
            state: Ok(RequestBody {
                query: document.to_owned(),
                variables: None,
                operation_name: None,
            }),
        }
    }
}

fn sensitive(value: &str) -> Result<HeaderValue, reqwest::header::InvalidHeaderValue> {
    let mut header = HeaderValue::from_str(value)?;
    header.set_sensitive(true);
    Ok(header)
}

#[derive(Serialize)]
struct RequestBody {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<Box<RawValue>>,
    #[serde(rename = "operationName", skip_serializing_if = "Option::is_none")]
    operation_name: Option<String>,
}

/// Buffered GraphQL request. Serialization/header failures surface when sent.
#[must_use = "call send or fetch to run the request"]
pub struct QueryBuilder {
    http: reqwest::Client,
    endpoint: Url,
    headers: HeaderMap,
    state: Result<RequestBody, GraphqlClientError>,
    retry_policy: RetryPolicy,
}

impl fmt::Debug for QueryBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QueryBuilder")
            .finish_non_exhaustive()
    }
}

impl QueryBuilder {
    fn update(
        mut self,
        apply: impl FnOnce(&mut RequestBody) -> Result<(), GraphqlClientError>,
    ) -> Self {
        if let Ok(body) = self.state.as_mut()
            && let Err(error) = apply(body)
        {
            self.state = Err(error);
        }
        self
    }

    /// Serialize variables once and retain their raw JSON until request serialization.
    pub fn variables<T: Serialize + ?Sized>(self, variables: &T) -> Self {
        self.update(|body| {
            body.variables = Some(
                serde_json::value::to_raw_value(variables)
                    .map_err(GraphqlClientError::Serialize)?,
            );
            Ok(())
        })
    }

    /// Select an operation in a multi-operation document.
    pub fn operation_name(self, name: &str) -> Self {
        self.update(|body| {
            body.operation_name = Some(name.to_owned());
            Ok(())
        })
    }

    /// Replace a request header. All custom values are treated as sensitive.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        if self.state.is_ok() {
            let result = HeaderName::from_bytes(name.as_bytes())
                .map_err(GraphqlClientError::from)
                .and_then(|name| Ok((name, sensitive(value)?)));
            match result {
                Ok((name, value)) => {
                    self.headers.insert(name, value);
                }
                Err(error) => self.state = Err(error),
            }
        }
        self
    }

    /// Merge request headers, marking every supplied value sensitive.
    pub fn headers(mut self, mut headers: HeaderMap) -> Self {
        for value in headers.values_mut() {
            value.set_sensitive(true);
        }
        self.headers.extend(headers);
        self
    }

    /// Send and return the raw successful HTTP response, without interpreting execution errors.
    ///
    /// # Errors
    /// Returns construction, serialization, transport, or non-2xx HTTP errors.
    pub async fn send(self) -> Result<reqwest::Response, GraphqlClientError> {
        let body = serde_json::to_vec(&self.state?).map_err(GraphqlClientError::Serialize)?;
        let builder = self
            .http
            .post(self.endpoint)
            .headers(self.headers)
            .header(CONTENT_TYPE, "application/json")
            .body(body);
        let (http, request) = builder.build_split();
        let response = rp_supabase_http::execute(&http, request?, self.retry_policy).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(GraphqlClientError::Http {
                status,
                body: response.text().await?,
            });
        }
        Ok(response)
    }

    /// Decode typed data and preserve execution errors, including null/partial data.
    ///
    /// # Errors
    /// Returns send errors, malformed envelopes, or data decoding errors.
    pub async fn fetch<T: DeserializeOwned>(
        self,
    ) -> Result<GraphqlResponse<T>, GraphqlClientError> {
        let bytes = self.send().await?.bytes().await?;
        let envelope: Envelope =
            serde_json::from_slice(&bytes).map_err(GraphqlClientError::Decode)?;
        if envelope.data.is_none() && envelope.errors.is_empty() {
            return Err(GraphqlClientError::EmptyEnvelope);
        }
        let data = envelope
            .data
            .map(|raw| serde_json::from_str::<Option<T>>(raw.get()))
            .transpose()
            .map_err(GraphqlClientError::Decode)?
            .flatten();
        Ok(GraphqlResponse {
            data,
            errors: envelope.errors,
            extensions: envelope.extensions,
        })
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default, deserialize_with = "present_data")]
    data: Option<Box<RawValue>>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
    #[serde(default)]
    extensions: Option<serde_json::Value>,
}

fn present_data<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(deserializer).map(Some)
}
