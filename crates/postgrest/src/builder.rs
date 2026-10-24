// Adapted from postgrest-rs / rp-postgrest 2.1.0 (MIT OR Apache-2.0).
use crate::{ConfigError, Configuration, Count, Counted, Error, ResponseMetadata};
use alloc::{borrow::Cow, sync::Arc};
use reqwest::{
    Method, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};

/// A fluent request with private state and deferred serialization/configuration errors.
#[derive(Debug)]
#[must_use]
pub struct Builder {
    configuration: Arc<Configuration>,
    url: Url,
    method: Method,
    rpc: bool,
    schema: Option<String>,
    queries: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    headers: HeaderMap,
    preferences: Vec<(String, String)>,
    body: Option<String>,
    error: Option<Error>,
}
impl Builder {
    pub(crate) fn new(configuration: Arc<Configuration>, name: &str, rpc: bool) -> Self {
        let mut url = configuration.base.clone();
        let error = matches!(name, "." | "..").then(|| ConfigError::DotOnlyResource.into());
        // Encode punctuation, including dots, to preserve literal resource identity.
        let mut encoded = String::with_capacity(name.len());
        for byte in name.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
                encoded.push(char::from(byte));
            } else {
                encoded.push('%');
                for nibble in [byte >> 4_u8, byte & 0x0F] {
                    let digit = if nibble < 10 {
                        b'0'.wrapping_add(nibble)
                    } else {
                        b'A'.wrapping_add(nibble.wrapping_sub(10))
                    };
                    encoded.push(char::from(digit));
                }
            }
        }
        let path = format!("{}{}{encoded}", url.path(), if rpc { "rpc/" } else { "" });
        url.set_path(&path);
        let mut builder = Self {
            configuration,
            url,
            method: Method::GET,
            rpc,
            schema: None,
            queries: Vec::new(),
            headers: HeaderMap::new(),
            preferences: Vec::new(),
            body: None,
            error,
        };
        let inherited = Arc::clone(&builder.configuration);
        for preference in &inherited.headers.get_all("prefer") {
            match preference.to_str() {
                Ok(preference) => builder.merge_preferences(preference),
                Err(_) => builder.defer(ConfigError::InvalidPreference.into()),
            }
        }
        builder
    }
    fn defer(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    fn merge_preferences(&mut self, value: &str) {
        for directive in value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let key = directive
                .split_once('=')
                .map_or(directive, |(key, _)| key)
                .trim();
            if let Some((_, previous)) = self
                .preferences
                .iter_mut()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            {
                directive.clone_into(previous);
            } else {
                self.preferences
                    .push((key.to_owned(), directive.to_owned()));
            }
        }
    }
    /// Appends raw `PostgREST` grammar without rewriting or deduplicating it.
    pub fn append_query<K: Into<Cow<'static, str>>, V: Into<Cow<'static, str>>>(
        &mut self,
        key: K,
        value: V,
    ) -> &mut Self {
        self.queries.push((key.into(), value.into()));
        self
    }
    /// Reads the ordered raw query pairs.
    pub fn query_pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.queries
            .iter()
            .map(|(key, value)| (key.as_ref(), value.as_ref()))
    }
    /// Sets a request-local bearer token, deferring invalid values.
    pub fn auth<T: AsRef<str>>(self, token: T) -> Self {
        self.insert_header("authorization", format!("Bearer {}", token.as_ref()))
    }
    /// Sets a request header. Prefer directives compose by key, last value winning.
    pub fn insert_header<N: AsRef<str>, V: AsRef<str>>(mut self, name: N, value: V) -> Self {
        let parsed = HeaderName::from_bytes(name.as_ref().as_bytes())
            .map_err(ConfigError::HeaderName)
            .and_then(|name| {
                HeaderValue::from_str(value.as_ref())
                    .map(|value| (name, value))
                    .map_err(ConfigError::HeaderValue)
            });
        match parsed {
            Ok((name, mut header)) => {
                if matches!(name.as_str(), "authorization" | "apikey") {
                    header.set_sensitive(true);
                }
                if name.as_str() == "prefer" {
                    self.merge_preferences(value.as_ref());
                } else {
                    self.headers.insert(name, header);
                }
            }
            Err(error) => self.defer(error.into()),
        }
        self
    }
    /// Overrides the HTTP method, including HEAD and RPC GET calls.
    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }
    /// Sets a request-local schema profile.
    pub fn schema<S: AsRef<str>>(mut self, schema: S) -> Self {
        self.schema = Some(schema.as_ref().to_owned());
        self
    }
    /// Appends a projection without allocating borrowed static grammar.
    pub fn select<S: Into<Cow<'static, str>>>(mut self, selection: S) -> Self {
        self.append_query("select", selection);
        self
    }
    fn compose_order(&mut self, key: Cow<'static, str>, value: String) {
        if let Some((_, existing)) = self
            .queries
            .iter_mut()
            .find(|(candidate, _)| candidate == &key)
        {
            let existing = existing.to_mut();
            existing.push(',');
            existing.push_str(&value);
        } else {
            self.append_query(key, value);
        }
    }
    /// Composes ordering into one effective order parameter.
    pub fn order<C: Into<String>>(mut self, columns: C) -> Self {
        self.compose_order(Cow::Borrowed("order"), columns.into());
        self
    }
    /// Composes foreign or top-level ordering with explicit direction and null placement.
    pub fn order_with_options<T: Into<String>, U: Into<String>>(
        mut self,
        columns: T,
        foreign_table: Option<U>,
        ascending: bool,
        nulls_first: bool,
    ) -> Self {
        let key = foreign_table
            .map(Into::into)
            .filter(|table| !table.is_empty())
            .map_or(Cow::Borrowed("order"), |table| {
                Cow::Owned(format!("{table}.order"))
            });
        self.compose_order(
            key,
            format!(
                "{}.{}.{}",
                columns.into(),
                if ascending { "asc" } else { "desc" },
                if nulls_first {
                    "nullsfirst"
                } else {
                    "nullslast"
                }
            ),
        );
        self
    }
    /// Requests an explicit row limit; zero requests zero rows. Does not imply safe limited mutations.
    pub fn limit(mut self, count: usize) -> Self {
        self.append_query("limit", count.to_string());
        self
    }
    /// Limits a foreign relation without changing top-level pagination.
    pub fn foreign_table_limit<F: Into<String>>(mut self, count: usize, foreign_table: F) -> Self {
        self.append_query(format!("{}.limit", foreign_table.into()), count.to_string());
        self
    }
    /// Requests an inclusive item range; does not imply safe limited mutations.
    pub fn range(self, low: usize, high: usize) -> Self {
        self.insert_header("range-unit", "items")
            .insert_header("range", format!("{low}-{high}"))
    }
    /// Requests a server count without changing pagination or unrelated preferences.
    pub fn count(self, count: Count) -> Self {
        self.insert_header("prefer", count.preference())
    }
    /// Requests no mutation representation, preserving unrelated preferences.
    pub fn return_minimal(self) -> Self {
        self.insert_header("prefer", "return=minimal")
    }
    /// Requests an exact count without changing pagination.
    pub fn exact_count(self) -> Self {
        self.count(Count::Exact)
    }
    /// Requests a planned count without changing pagination.
    pub fn planned_count(self) -> Self {
        self.count(Count::Planned)
    }
    /// Requests an estimated count without changing pagination.
    pub fn estimated_count(self) -> Self {
        self.count(Count::Estimated)
    }
    /// Requests server-enforced single-object cardinality.
    pub fn single(self) -> Self {
        self.insert_header("accept", "application/vnd.pgrst.object+json")
    }
    /// Inserts unmodified raw JSON and requests representation.
    pub fn insert<B: Into<String>>(mut self, body: B) -> Self {
        self.method = Method::POST;
        self.body = Some(body.into());
        self.insert_header("prefer", "return=representation")
    }
    /// Upserts raw JSON, merging duplicates and requesting representation.
    pub fn upsert<B: Into<String>>(self, body: B) -> Self {
        self.insert(body)
            .insert_header("prefer", "resolution=merge-duplicates")
    }
    /// Specifies the unique columns used to resolve an upsert conflict.
    pub fn on_conflict<C: Into<Cow<'static, str>>>(mut self, columns: C) -> Self {
        self.append_query("on_conflict", columns);
        self
    }
    /// Updates with unmodified raw JSON and requests representation.
    pub fn update<B: Into<String>>(mut self, body: B) -> Self {
        self.method = Method::PATCH;
        self.body = Some(body.into());
        self.insert_header("prefer", "return=representation")
    }
    /// Deletes rows and requests representation.
    pub fn delete(mut self) -> Self {
        self.method = Method::DELETE;
        self.insert_header("prefer", "return=representation")
    }
    /// Uses a raw RPC body without adding relation preferences or cardinality.
    pub fn rpc<B: Into<String>>(mut self, body: B) -> Self {
        self.method = Method::POST;
        self.body = Some(body.into());
        self
    }
    fn serialize<T: Serialize + ?Sized>(&mut self, payload: &T) {
        match serde_json::to_string(payload) {
            Ok(body) => self.body = Some(body),
            Err(error) => self.defer(Error::Serialization(error)),
        }
    }
    /// Inserts JSON, retaining the first serialization failure until build.
    pub fn insert_json<T: Serialize + ?Sized>(mut self, payload: &T) -> Self {
        self.method = Method::POST;
        self.serialize(payload);
        self.insert_header("prefer", "return=representation")
    }
    /// Upserts JSON, retaining the first serialization failure until build.
    pub fn upsert_json<T: Serialize + ?Sized>(self, payload: &T) -> Self {
        self.insert_json(payload)
            .insert_header("prefer", "resolution=merge-duplicates")
    }
    /// Updates JSON, retaining the first serialization failure until build.
    pub fn update_json<T: Serialize + ?Sized>(mut self, payload: &T) -> Self {
        self.method = Method::PATCH;
        self.serialize(payload);
        self.insert_header("prefer", "return=representation")
    }
    /// Sets JSON RPC arguments without relation select/single semantics.
    pub fn rpc_json<T: Serialize + ?Sized>(mut self, payload: &T) -> Self {
        self.method = Method::POST;
        self.serialize(payload);
        self
    }
    /// Validates deferred state and returns a customizable Reqwest request builder.
    ///
    /// # Errors
    /// Returns deferred serialization or header/resource configuration failures.
    pub fn build(self) -> Result<reqwest::RequestBuilder, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut headers = self.configuration.headers.clone();
        headers.remove("prefer");
        if !headers.contains_key("accept") {
            headers.insert("accept", HeaderValue::from_static("application/json"));
        }
        for (name, value) in &self.headers {
            headers.insert(name.clone(), value.clone());
        }
        if !self.preferences.is_empty() {
            let capacity = self
                .preferences
                .iter()
                .fold(0_usize, |capacity, (_, value)| {
                    capacity.saturating_add(value.len()).saturating_add(1)
                });
            let mut preference = String::with_capacity(capacity);
            for (_, directive) in &self.preferences {
                if !preference.is_empty() {
                    preference.push(',');
                }
                preference.push_str(directive);
            }
            headers.insert(
                "prefer",
                HeaderValue::from_str(&preference).map_err(ConfigError::HeaderValue)?,
            );
        }
        if let Some(schema) = self.schema.as_ref().or(self.configuration.schema.as_ref()) {
            let key = if matches!(self.method, Method::GET | Method::HEAD) {
                "accept-profile"
            } else {
                "content-profile"
            };
            headers.insert(
                key,
                HeaderValue::from_str(schema).map_err(ConfigError::HeaderValue)?,
            );
        }
        if !matches!(self.method, Method::GET | Method::HEAD)
            && !headers.contains_key("content-type")
        {
            headers.insert("content-type", HeaderValue::from_static("application/json"));
        }
        let request = self
            .configuration
            .client
            .request(self.method, self.url)
            .headers(headers)
            .query(&self.queries);
        Ok(if let Some(body) = self.body {
            request.body(body)
        } else {
            request
        })
    }
    /// Executes without interpreting status or consuming the response body.
    ///
    /// # Errors
    /// Returns configuration, serialization, or transport errors.
    pub async fn execute(self) -> Result<reqwest::Response, Error> {
        let policy = self.configuration.retry_policy;
        let (http, request) = self.build()?.build_split();
        rp_supabase_http::execute(&http, request.map_err(Error::Request)?, policy)
            .await
            .map_err(Error::Request)
    }
    /// Rejects every non-2xx response, including HTTP 300. Success remains untouched.
    ///
    /// # Errors
    /// Returns request, body-read, structured server, or malformed-envelope errors.
    pub async fn execute_checked(self) -> Result<reqwest::Response, Error> {
        let mut response = self.execute().await?;
        if response.status().is_success() {
            return Ok(response);
        }
        let metadata = Box::new(ResponseMetadata::take_from_response(&mut response));
        let body = match response.bytes().await {
            Ok(body) => body,
            Err(source) => return Err(Error::ResponseBody { metadata, source }),
        };
        Err(Error::decode_server_response(metadata, &body))
    }
    /// Checks HTTP success, then centrally decodes the requested response shape.
    ///
    /// HTTP 204 has no representation and uses Serde's unit deserializer.
    /// Empty HTTP 200 bodies remain invalid JSON.
    ///
    /// # Errors
    /// Also retains success metadata for body-read or JSON/shape decode failures.
    pub async fn fetch<T: DeserializeOwned>(self) -> Result<T, Error> {
        crate::response::decode(self.execute_checked().await?)
            .await
            .map(|(data, _)| data)
    }
    /// Decodes a representation and returns its server total and response metadata.
    ///
    /// # Errors
    /// Returns ordinary fetch errors or a typed count failure with response metadata.
    pub async fn fetch_with_count<T: DeserializeOwned>(
        self,
        count: Count,
    ) -> Result<Counted<T>, Error> {
        let (data, metadata) =
            crate::response::decode(self.count(count).execute_checked().await?).await?;
        match metadata.count() {
            Ok(count) => Ok(Counted {
                data,
                count,
                metadata,
            }),
            Err(source) => Err(Error::Count {
                metadata: Box::new(metadata),
                source,
            }),
        }
    }
    /// Requests only the server total. Relation GET requests use HEAD; mutations
    /// retain their method and request minimal return. RPC methods remain unchanged.
    ///
    /// # Errors
    /// Returns checked execution errors or a typed count failure with response metadata.
    pub async fn execute_count(mut self, count: Count) -> Result<u64, Error> {
        if self.method == Method::GET && !self.rpc {
            self.method = Method::HEAD;
        } else if !matches!(self.method, Method::GET | Method::HEAD) {
            self = self.return_minimal();
        } else {
            // Read-only GET/HEAD calls already request no mutation representation.
        }
        let mut response = self.count(count).execute_checked().await?;
        crate::count::response_count(response.headers()).map_err(|source| Error::Count {
            metadata: Box::new(ResponseMetadata::take_from_response(&mut response)),
            source,
        })
    }
}
