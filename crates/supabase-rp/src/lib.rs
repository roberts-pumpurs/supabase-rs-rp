#![cfg_attr(doc, doc = include_str!("../README.md"))]

/// `PostgREST` query builder (`rp-postgrest`).
#[cfg(feature = "rest")]
pub use rp_postgrest as postgrest;
/// Supabase Auth API client and token streams (`rp-supabase-auth`).
#[cfg(feature = "auth")]
pub use rp_supabase_auth as auth;
/// Typed runtime for generated schemas (`rp-supabase-client`).
///
/// The `typed` feature enables only the schema runtime (`typed::schema`). Generate code with
/// the codegen `runtime_path` set to `::supabase_rp::typed::schema`. The `typed-client`
/// feature also enables the client API of `rp-supabase-client` (authentication and response
/// decoding), which turns on `serde_json/arbitrary_precision`.
#[cfg(feature = "typed")]
pub use rp_supabase_client as typed;
/// Edge Functions client (`rp-supabase-functions`).
#[cfg(feature = "functions")]
pub use rp_supabase_functions as functions;
/// GraphQL queries, mutations, and partial responses (`rp-supabase-graphql`).
#[cfg(feature = "graphql")]
pub use rp_supabase_graphql as graphql;
pub use rp_supabase_http::RetryPolicy;
/// Realtime changes, broadcast, and presence (`rp-supabase-realtime`).
#[cfg(feature = "realtime")]
pub use rp_supabase_realtime as realtime;
/// Storage buckets and objects (`rp-supabase-storage`).
#[cfg(feature = "storage")]
pub use rp_supabase_storage as storage;
pub use url::Url;

/// Header that carries the project API key.
#[cfg(feature = "rest")]
const API_KEY_HEADER: &str = "apikey";

/// Maximum sign-in and token refresh retries in [`Client::realtime_config`].
#[cfg(feature = "realtime")]
pub const REALTIME_MAX_RECONNECT_ATTEMPTS: u8 = 5;

/// Delay between sign-in and token refresh retries in [`Client::realtime_config`].
#[cfg(feature = "realtime")]
pub const REALTIME_RECONNECT_INTERVAL: core::time::Duration = core::time::Duration::from_secs(3);

/// Errors from building a [`Client`].
#[derive(Debug, thiserror::Error)]
#[expect(
    clippy::error_impl_error,
    reason = "the crate exposes one error type named `Error`"
)]
#[non_exhaustive]
pub enum Error {
    /// The project URL does not parse.
    #[error("invalid project URL: {0}")]
    UrlParse(#[from] url::ParseError),
    /// The project URL parses but is not a usable project base URL.
    #[error("invalid project URL: {0}")]
    InvalidProjectUrl(&'static str),
    /// The REST client rejected its configuration.
    #[cfg(feature = "rest")]
    #[error(transparent)]
    Rest(#[from] rp_postgrest::Error),
    /// The auth client rejected its configuration.
    #[cfg(feature = "auth")]
    #[error(transparent)]
    Auth(#[from] rp_supabase_auth::error::AuthError),
    /// The storage client rejected its configuration.
    #[cfg(feature = "storage")]
    #[error(transparent)]
    Storage(#[from] rp_supabase_storage::StorageError),
    /// The functions client rejected its configuration.
    #[cfg(feature = "functions")]
    #[error(transparent)]
    Functions(#[from] rp_supabase_functions::FunctionsError),
    /// The GraphQL client rejected its configuration.
    #[cfg(feature = "graphql")]
    #[error(transparent)]
    Graphql(#[from] rp_supabase_graphql::GraphqlClientError),
    /// The default HTTP client cannot be built.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

/// Client for one Supabase project.
///
/// It holds one shared `reqwest::Client` and one sub-client per enabled feature.
/// Cloning is cheap: the connection pool is shared.
#[derive(Clone)]
pub struct Client {
    project_url: Url,
    api_key: String,
    http: reqwest::Client,
    retry_policy: RetryPolicy,
    #[cfg(feature = "rest")]
    rest: rp_postgrest::Postgrest,
    #[cfg(feature = "auth")]
    auth: rp_supabase_auth::auth_client::ApiClient,
    #[cfg(feature = "storage")]
    storage: rp_supabase_storage::StorageClient,
    #[cfg(feature = "functions")]
    functions: rp_supabase_functions::FunctionsClient,
    #[cfg(feature = "graphql")]
    graphql: rp_supabase_graphql::GraphqlClient,
}

impl core::fmt::Debug for Client {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Client")
            .field("project_url", &self.project_url.as_str())
            .field("api_key", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Creates a client for the project at `project_url`, for example `https://abc.supabase.co/`.
    ///
    /// The default HTTP client follows up to 10 redirects, only to the same origin.
    ///
    /// # Errors
    /// Returns [`Error::UrlParse`] or [`Error::InvalidProjectUrl`] for a bad URL, and a
    /// sub-client error when a credential is not a valid header value.
    pub fn new(project_url: &str, api_key: &str) -> Result<Self, Error> {
        let http = rp_supabase_http::client_builder().build()?;
        Self::new_with_client(project_url, api_key, http)
    }

    /// Creates a client that reuses the connection pool and policies of `http`.
    /// The redirect policy of `http` applies unchanged.
    ///
    /// # Errors
    /// Same as [`Client::new`].
    pub fn new_with_client(
        project_url: &str,
        api_key: &str,
        http: reqwest::Client,
    ) -> Result<Self, Error> {
        let project_url = parse_project_url(project_url)?;
        Ok(Self {
            #[cfg(feature = "rest")]
            rest: rest_client(&project_url, api_key, default_bearer(api_key), http.clone())?,
            #[cfg(feature = "auth")]
            auth: auth_client(&project_url, api_key, default_bearer(api_key), http.clone())?,
            #[cfg(feature = "storage")]
            storage: rp_supabase_storage::StorageClient::new_with_client(
                &project_url,
                api_key,
                http.clone(),
            )?,
            #[cfg(feature = "functions")]
            functions: rp_supabase_functions::FunctionsClient::new_with_client(
                &project_url,
                api_key,
                http.clone(),
            )?,
            #[cfg(feature = "graphql")]
            graphql: rp_supabase_graphql::GraphqlClient::new_with_client(
                &project_url,
                api_key,
                http.clone(),
            )?,
            retry_policy: RetryPolicy::default(),
            api_key: api_key.to_owned(),
            http,
            project_url,
        })
    }

    /// Returns a client that acts as the user who owns `token` (a JWT from sign-in).
    ///
    /// REST, auth, storage, and functions send `Authorization: Bearer <token>`.
    /// The `apikey` header keeps the project key. The connection pool is shared.
    /// Without a user token, a legacy JWT key (anon or `service_role`) is the bearer, as in
    /// supabase-js; a new-format key (`sb_publishable_...`, `sb_secret_...`) is sent only as
    /// `apikey`.
    ///
    /// # Errors
    /// Returns a sub-client error when `token` is not a valid header value.
    #[cfg_attr(
        not(any(
            feature = "rest",
            feature = "auth",
            feature = "storage",
            feature = "functions",
            feature = "graphql"
        )),
        expect(
            unused_variables,
            clippy::unnecessary_wraps,
            reason = "no enabled service uses a token"
        )
    )]
    pub fn with_access_token(&self, token: &str) -> Result<Self, Error> {
        Ok(Self {
            project_url: self.project_url.clone(),
            api_key: self.api_key.clone(),
            http: self.http.clone(),
            retry_policy: self.retry_policy,
            #[cfg(feature = "rest")]
            rest: rest_client(
                &self.project_url,
                &self.api_key,
                Some(token),
                self.http.clone(),
            )?
            .with_retry_policy(self.retry_policy),
            #[cfg(feature = "auth")]
            auth: auth_client(
                &self.project_url,
                &self.api_key,
                Some(token),
                self.http.clone(),
            )?
            .with_retry_policy(self.retry_policy),
            #[cfg(feature = "storage")]
            storage: self.storage.with_access_token(token)?,
            #[cfg(feature = "functions")]
            functions: self.functions.with_access_token(token)?,
            #[cfg(feature = "graphql")]
            graphql: self.graphql.with_access_token(token)?,
        })
    }

    /// Apply one rate-limit retry policy to every enabled HTTP client.
    /// Mutations are sent once unless the policy explicitly permits replay.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        #[cfg(feature = "rest")]
        {
            self.rest = self.rest.with_retry_policy(policy);
        }
        #[cfg(feature = "auth")]
        {
            self.auth = self.auth.with_retry_policy(policy);
        }
        #[cfg(feature = "storage")]
        {
            self.storage = self.storage.with_retry_policy(policy);
        }
        #[cfg(feature = "functions")]
        {
            self.functions = self.functions.with_retry_policy(policy);
        }
        #[cfg(feature = "graphql")]
        {
            self.graphql = self.graphql.with_retry_policy(policy);
        }
        self
    }

    /// Returns a client that sends storage requests to the project at `project_url`, for
    /// example `https://abc.supabase.co/`. REST, auth, and functions keep the URL from
    /// [`Client::new`].
    ///
    /// Use it when a custom domain proxies REST and auth but not storage.
    /// The storage client keeps its headers, so an access token from
    /// [`Client::with_access_token`] stays. Call order does not matter.
    ///
    /// # Errors
    /// Returns [`Error::UrlParse`] or [`Error::InvalidProjectUrl`] for a bad URL.
    #[cfg(feature = "storage")]
    pub fn with_storage_url(&self, project_url: &str) -> Result<Self, Error> {
        let storage_url = parse_project_url(project_url)?;
        let mut next = self.clone();
        next.storage = self.storage.with_project_url(&storage_url)?;
        Ok(next)
    }

    /// Project base URL.
    #[must_use]
    pub const fn project_url(&self) -> &Url {
        &self.project_url
    }

    /// REST client for `{project}/rest/v1/`.
    ///
    /// The typed runtime of `rp-supabase-client` takes this same `Postgrest`.
    #[cfg(feature = "rest")]
    #[must_use]
    pub const fn rest(&self) -> &rp_postgrest::Postgrest {
        &self.rest
    }

    /// Starts a query on `table`. Shortcut for `self.rest().from(table)`.
    #[cfg(feature = "rest")]
    pub fn from(&self, table: &str) -> rp_postgrest::Builder {
        self.rest.from(table)
    }

    /// Supabase Auth client for `{project}/auth/v1/`.
    ///
    /// After [`Client::with_access_token`] it is authenticated, so `get_user`,
    /// `update_user`, and `sign_out` work.
    #[cfg(feature = "auth")]
    #[must_use]
    pub const fn auth(&self) -> &rp_supabase_auth::auth_client::ApiClient {
        &self.auth
    }

    /// Build an elevated admin client with explicit server-only credentials.
    /// This does not elevate or replace the user session on this client.
    ///
    /// # Errors
    /// Returns invalid service-role header errors.
    #[cfg(feature = "auth")]
    pub fn admin(
        &self,
        service_role_key: &str,
    ) -> Result<rp_supabase_auth::admin::AdminClient, Error> {
        Ok(rp_supabase_auth::admin::AdminClient::new_with_client(
            &self.project_url,
            service_role_key,
            self.http.clone(),
        )?
        .with_retry_policy(self.retry_policy))
    }

    /// Storage client for `{project}/storage/v1/`.
    #[cfg(feature = "storage")]
    #[must_use]
    pub const fn storage(&self) -> &rp_supabase_storage::StorageClient {
        &self.storage
    }

    /// Edge Functions client for `{project}/functions/v1/`.
    #[cfg(feature = "functions")]
    #[must_use]
    pub const fn functions(&self) -> &rp_supabase_functions::FunctionsClient {
        &self.functions
    }

    /// GraphQL client for `{project}/graphql/v1`.
    #[cfg(feature = "graphql")]
    #[must_use]
    pub const fn graphql(&self) -> &rp_supabase_graphql::GraphqlClient {
        &self.graphql
    }

    /// Configuration for `RealtimeConnection::db_changes`, `presence`, and `broadcast`.
    ///
    /// Realtime signs in on its own, so it uses the project key, not a user token.
    /// Retry defaults for sign-in and token refresh: [`REALTIME_MAX_RECONNECT_ATTEMPTS`]
    /// attempts, [`REALTIME_RECONNECT_INTERVAL`] apart. Websocket reconnect has its own
    /// options on `RealtimeConnection`.
    #[cfg(feature = "realtime")]
    #[must_use]
    pub fn realtime_config(
        &self,
    ) -> rp_supabase_realtime::rp_supabase_auth::jwt_stream::SupabaseAuthConfig {
        rp_supabase_realtime::rp_supabase_auth::jwt_stream::SupabaseAuthConfig {
            api_key: self.api_key.clone(),
            max_reconnect_attempts: REALTIME_MAX_RECONNECT_ATTEMPTS,
            reconnect_interval: REALTIME_RECONNECT_INTERVAL,
            url: self.project_url.clone(),
        }
    }
}

fn parse_project_url(input: &str) -> Result<Url, Error> {
    let url = Url::parse(input)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::InvalidProjectUrl("scheme must be http or https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidProjectUrl("credentials are not allowed"));
    }
    if url.query().is_some() {
        return Err(Error::InvalidProjectUrl("query is not allowed"));
    }
    if url.fragment().is_some() {
        return Err(Error::InvalidProjectUrl("fragment is not allowed"));
    }
    // Auth joins `/auth/v1/` from the host root, so a path prefix would route services apart.
    if url.path() != "/" {
        return Err(Error::InvalidProjectUrl(
            "path is not allowed; pass the project root URL",
        ));
    }
    Ok(url)
}

/// Returns the API key as the default bearer for legacy JWT keys. New-format keys
/// (`sb_publishable_`, `sb_secret_`) are not JWTs, so supabase-js sends them only as `apikey`.
#[cfg(any(feature = "rest", feature = "auth"))]
fn default_bearer(api_key: &str) -> Option<&str> {
    let new_format = api_key.starts_with("sb_publishable_") || api_key.starts_with("sb_secret_");
    (!new_format).then_some(api_key)
}

/// Builds the auth client, authenticated when a bearer is given.
#[cfg(feature = "auth")]
fn auth_client(
    project_url: &Url,
    api_key: &str,
    bearer: Option<&str>,
    http: reqwest::Client,
) -> Result<rp_supabase_auth::auth_client::ApiClient, Error> {
    use rp_supabase_auth::auth_client::ApiClient;
    Ok(match bearer {
        Some(bearer) => {
            ApiClient::new_authenticated_with_client(project_url, api_key, bearer, http)?
        }
        None => ApiClient::new_unauthenticated_with_client(project_url, api_key, http)?,
    })
}

/// Builds the REST client like `rp_supabase_client::anonymous_client_with_client`, plus an
/// optional bearer.
#[cfg(feature = "rest")]
fn rest_client(
    project_url: &Url,
    api_key: &str,
    bearer: Option<&str>,
    http: reqwest::Client,
) -> Result<rp_postgrest::Postgrest, Error> {
    let base = project_url.join("rest/v1/")?;
    let client = rp_postgrest::Postgrest::new_with_client(base.as_str(), http)?
        .insert_header(API_KEY_HEADER, api_key)?;
    Ok(match bearer {
        Some(bearer) => client.auth(bearer)?,
        None => client,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_project_urls() {
        for input in [
            "not a url",
            "ftp://abc.supabase.co/",
            "https://user:pass@abc.supabase.co/",
            "https://abc.supabase.co/?x=1",
            "https://abc.supabase.co/#frag",
            "https://gateway.example/supabase",
        ] {
            assert!(
                Client::new(input, "key").is_err(),
                "{input} must be rejected"
            );
        }
    }
}
