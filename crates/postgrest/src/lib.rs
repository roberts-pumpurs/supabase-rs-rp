#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![expect(
    clippy::multiple_inherent_impl,
    reason = "Raw filter grammar and request execution have separate modules"
)]
// Adapted from postgrest-rs / rp-postgrest 2.1.0 (MIT OR Apache-2.0).
extern crate alloc;
mod builder;
mod count;
mod error;
mod filter;
mod response;

pub use builder::Builder;
pub use count::{Count, CountError, Counted};
pub use error::{ConfigError, Error, ResponseMetadata};
pub use reqwest;
pub use rp_postgrest_error;
pub use rp_supabase_http::RetryPolicy;

use alloc::sync::Arc;
use reqwest::{
    Client, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::Serialize;

#[derive(Clone, Debug)]
struct Configuration {
    base: Url,
    schema: Option<String>,
    headers: HeaderMap,
    client: Client,
    retry_policy: RetryPolicy,
}

/// An immutable client configuration sharing one HTTP connection pool.
#[derive(Clone, Debug)]
pub struct Postgrest {
    configuration: Arc<Configuration>,
}
impl Postgrest {
    /// Creates a client with fallible default transport construction.
    ///
    /// # Errors
    /// Returns configuration failures for an invalid base URL or HTTP client.
    pub fn new<U: AsRef<str>>(base_url: U) -> Result<Self, Error> {
        let client = rp_supabase_http::client_builder()
            .build()
            .map_err(ConfigError::Client)?;
        Self::new_with_client(base_url, client)
    }
    /// Reuses the supplied transport's pooling, timeout, proxy and TLS policy.
    ///
    /// # Errors
    /// Rejects non-HTTP(S) bases, credentials, queries and fragments.
    pub fn new_with_client<U: AsRef<str>>(base_url: U, client: Client) -> Result<Self, Error> {
        let mut base = Url::parse(base_url.as_ref()).map_err(ConfigError::Url)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(ConfigError::InvalidBaseUrl.into());
        }
        let path = format!("{}/", base.path().trim_end_matches('/'));
        base.set_path(&path);
        Ok(Self {
            configuration: Arc::new(Configuration {
                base,
                schema: None,
                headers: HeaderMap::new(),
                client,
                retry_policy: RetryPolicy::default(),
            }),
        })
    }
    /// Adds a bearer token.
    ///
    /// # Errors
    /// Rejects invalid header values.
    pub fn auth<T: AsRef<str>>(self, token: T) -> Result<Self, Error> {
        self.insert_header("authorization", format!("Bearer {}", token.as_ref()))
    }
    /// Sets a header without changing the supplied HTTP transport.
    ///
    /// # Errors
    /// Rejects invalid header names or values.
    pub fn insert_header<N: AsRef<str>, V: AsRef<str>>(
        mut self,
        name: N,
        value: V,
    ) -> Result<Self, Error> {
        let name =
            HeaderName::from_bytes(name.as_ref().as_bytes()).map_err(ConfigError::HeaderName)?;
        let mut value = HeaderValue::from_str(value.as_ref()).map_err(ConfigError::HeaderValue)?;
        if matches!(name.as_str(), "authorization" | "apikey") {
            value.set_sensitive(true);
        }
        Arc::make_mut(&mut self.configuration)
            .headers
            .insert(name, value);
        Ok(self)
    }
    /// Sets the inherited schema, validated when a request is built.
    #[must_use]
    pub fn schema<S: AsRef<str>>(mut self, schema: S) -> Self {
        Arc::make_mut(&mut self.configuration).schema = Some(schema.as_ref().to_owned());
        self
    }
    /// Set the policy for explicit HTTP 429 responses on future requests.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        Arc::make_mut(&mut self.configuration).retry_policy = policy;
        self
    }
    /// Addresses a literal SQL resource name, encoding it exactly once.
    /// Dot-only names are rejected at build time because URL parsers normalize them.
    pub fn from<T: AsRef<str>>(&self, literal_table: T) -> Builder {
        Builder::new(
            Arc::clone(&self.configuration),
            literal_table.as_ref(),
            false,
        )
    }
    /// Calls a literal function name with an unmodified raw JSON body.
    pub fn rpc<F: AsRef<str>, B: Into<String>>(&self, literal_function: F, body: B) -> Builder {
        Builder::new(
            Arc::clone(&self.configuration),
            literal_function.as_ref(),
            true,
        )
        .rpc(body)
    }
    /// Calls a function, deferring a serialization failure until build/execution.
    pub fn rpc_json<T: Serialize + ?Sized, F: AsRef<str>>(
        &self,
        literal_function: F,
        args: &T,
    ) -> Builder {
        Builder::new(
            Arc::clone(&self.configuration),
            literal_function.as_ref(),
            true,
        )
        .rpc_json(args)
    }
}
