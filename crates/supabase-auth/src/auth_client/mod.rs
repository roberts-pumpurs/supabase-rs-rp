pub mod requests;
use core::marker::PhantomData;

use futures::{Stream, StreamExt as _};
use requests::{
    AuthModuleRequest, LogoutRequest, OtpRequest, RecoverRequest, SignupRequest, TokenRequest,
    UserGetRequest, UserUpdateRequest, VerifyPostRequest,
};
use reqwest::header;
use tracing::instrument;

use crate::error::AuthError;
use crate::jwt_stream::{RefreshStreamError, SupabaseAuthConfig};
use crate::types::{
    AccessTokenResponseSchema, ErrorSchema, LoginCredentials, OtpResponse, PkceGrant,
    RefreshTokenGrant, SignupPayload, SignupResponse, UserSchema,
};
use crate::{SUPABASE_KEY, jwt_stream};

#[derive(Clone, Debug)]
pub struct ApiClient {
    inner: reqwest::Client,
    url: url::Url,
    headers: header::HeaderMap,
    retry_policy: rp_supabase_http::RetryPolicy,
}

/// Creates a new authenticated stream.
///
/// # Errors
///
/// Returns an error if sign-in fails or the stream cannot be created.
pub fn new_authenticated_stream(
    config: SupabaseAuthConfig,
    login_info: LoginCredentials,
) -> Result<impl Stream<Item = Result<ApiClient, RefreshStreamError>>, RefreshStreamError> {
    new_authenticated_stream_with_client(config, login_info, default_client()?)
}

/// Creates an authenticated stream retaining one configured HTTP transport.
///
/// # Errors
/// Returns URL, credential header or sign-in setup failures.
pub fn new_authenticated_stream_with_client(
    config: SupabaseAuthConfig,
    login_info: LoginCredentials,
    http: reqwest::Client,
) -> Result<impl Stream<Item = Result<ApiClient, RefreshStreamError>>, RefreshStreamError> {
    let url = config.url.clone();
    let api_key = config.api_key.clone();
    let auth_stream = jwt_stream::JwtStream::new(config)
        .sign_in_with_client(login_info, http.clone())
        .map_err(RefreshStreamError::from)?;
    let client_stream = auth_stream.filter_map(move |item| {
        let client = match item {
            Ok(token) => token.access_token.as_deref().map(|access_token| {
                ApiClient::new_authenticated_with_client(&url, &api_key, access_token, http.clone())
                    .map_err(RefreshStreamError::from)
            }),
            Err(error) => Some(Err(error)),
        };
        futures::future::ready(client)
    });

    Ok(client_stream)
}

impl ApiClient {
    pub(crate) const fn auth_url(&self) -> &url::Url {
        &self.url
    }
    /// Configure bounded retries for explicit rate-limit responses.
    #[must_use]
    pub const fn with_retry_policy(mut self, policy: rp_supabase_http::RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub(crate) fn bearer_token(&self) -> Option<&str> {
        self.headers
            .get(header::AUTHORIZATION)?
            .to_str()
            .ok()?
            .strip_prefix("Bearer ")
    }

    /// Create an anonymous user, requiring a complete anonymous session response.
    ///
    /// # Errors
    /// Returns server errors or an invalid-session error for incomplete responses.
    pub async fn sign_in_anonymously(
        &self,
        data: Option<crate::types::UserMetadata>,
        captcha: Option<crate::types::GoTrueMetaSecurity>,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        let response = self
            .sign_up(SignupPayload {
                credentials: None,
                data,
                gotrue_meta_security: captcha,
                pkce: None,
            })
            .await?;
        let SignupResponse::Session(session) = response else {
            return Err(AuthError::InvalidSession(
                "anonymous signup returned no session",
            ));
        };
        crate::session_store::validate_session(&session)?;
        if session.user.as_ref().and_then(|user| user.is_anonymous) != Some(true) {
            return Err(AuthError::InvalidSession(
                "anonymous signup returned a non-anonymous user",
            ));
        }
        Ok(session)
    }
    /// Create a new unauthenticated API client.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be joined or the client cannot be created.
    pub fn new_unauthenticated(url: &url::Url, api_key: &str) -> Result<Self, AuthError> {
        Self::new_unauthenticated_with_client(url, api_key, default_client()?)
    }

    /// Create a new authenticated API client.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be joined or the client cannot be created.
    pub fn new_authenticated(
        url: &url::Url,
        api_key: &str,
        token: &str,
    ) -> Result<Self, AuthError> {
        Self::new_authenticated_with_client(url, api_key, token, default_client()?)
    }

    /// Create an unauthenticated client retaining the supplied HTTP transport.
    ///
    /// # Errors
    /// Returns URL or credential header failures.
    pub fn new_unauthenticated_with_client(
        url: &url::Url,
        api_key: &str,
        http: reqwest::Client,
    ) -> Result<Self, AuthError> {
        Ok(Self {
            url: url.join("/auth/v1/")?,
            inner: http,
            headers: base_headers(api_key)?,
            retry_policy: rp_supabase_http::RetryPolicy::default(),
        })
    }

    /// Create an authenticated client without altering shared transport defaults.
    ///
    /// # Errors
    /// Returns URL or credential header failures.
    pub fn new_authenticated_with_client(
        url: &url::Url,
        api_key: &str,
        token: &str,
        http: reqwest::Client,
    ) -> Result<Self, AuthError> {
        let mut client = Self::new_unauthenticated_with_client(url, api_key, http)?;
        let mut authorization = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
        authorization.set_sensitive(true);
        client.headers.insert(header::AUTHORIZATION, authorization);
        Ok(client)
    }

    /// Build a request for the API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request cannot be built.
    #[instrument(name = "build_request", skip(self, request))]
    pub fn build_request<T>(&self, request: &T) -> Result<Request<T::Res>, AuthError>
    where
        T: AuthModuleRequest + core::fmt::Debug,
    {
        let endpoint = request.path(&self.url)?;
        let method = T::METHOD;
        let client = self.inner.clone();
        let payload = simd_json::to_vec(&request.payload())?;
        let reqwest_req = client
            .request(method, endpoint.as_str())
            .headers(self.headers.clone())
            .body(payload);

        Ok(Request {
            builder: reqwest_req,
            result: PhantomData,
            retry_policy: self.retry_policy,
        })
    }

    /// Send a request and decode its successful response.
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] for a non-success status, or a build, transport or decode error.
    pub async fn send<T>(&self, request: &T) -> Result<T::Res, AuthError>
    where
        T: AuthModuleRequest + core::fmt::Debug,
    {
        self.build_request(request)?.execute().await?.json().await
    }

    async fn send_without_body<T>(&self, request: &T) -> Result<(), AuthError>
    where
        T: AuthModuleRequest + core::fmt::Debug,
    {
        self.build_request(request)?.execute().await?.ok().await
    }

    /// Create a user with `POST /signup`.
    ///
    /// When email confirmation is on, Supabase returns no session; the token fields are `None`.
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when Supabase rejects the sign-up.
    pub async fn sign_up(&self, payload: SignupPayload) -> Result<SignupResponse, AuthError> {
        self.send(&SignupRequest::builder().payload(payload).build())
            .await
    }

    /// Sign in with email or phone and a password (`POST /token?grant_type=password`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] for invalid credentials.
    pub async fn sign_in_with_password(
        &self,
        credentials: &LoginCredentials,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        self.send(&TokenRequest::Password(credentials.clone().into()))
            .await
    }

    /// Exchange a refresh token for a new session (`POST /token?grant_type=refresh_token`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when the refresh token is invalid or already used.
    pub async fn refresh_session(
        &self,
        refresh_token: &str,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        self.send(&TokenRequest::RefreshToken(RefreshTokenGrant {
            refresh_token: refresh_token.to_owned(),
        }))
        .await
    }

    /// Exchange the `code` from a PKCE OAuth or magic link redirect for a session
    /// (`POST /token?grant_type=pkce`). `code_verifier` is the verifier whose challenge started
    /// the flow.
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when the code is invalid, expired, or does not match the
    /// verifier.
    pub async fn exchange_code_for_session(
        &self,
        auth_code: &str,
        code_verifier: &str,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        self.send(&TokenRequest::Pkce(PkceGrant {
            auth_code: auth_code.to_owned(),
            code_verifier: code_verifier.to_owned(),
        }))
        .await
    }

    /// Send a one-time password or magic link (`POST /otp`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when Supabase rejects the request.
    pub async fn sign_in_with_otp(&self, request: &OtpRequest) -> Result<OtpResponse, AuthError> {
        self.send(request).await
    }

    /// Verify a one-time password or token hash and start a session (`POST /verify`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when the token is invalid or expired.
    pub async fn verify_otp(
        &self,
        request: &VerifyPostRequest,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        self.send(request).await
    }

    /// Send a password recovery email (`POST /recover`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when Supabase rejects the request.
    pub async fn reset_password_for_email(&self, email: &str) -> Result<(), AuthError> {
        self.send_without_body(&RecoverRequest::builder().email(email.to_owned()).build())
            .await
    }

    /// Fetch the user that owns the bearer token (`GET /user`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when the client has no valid access token.
    pub async fn get_user(&self) -> Result<UserSchema, AuthError> {
        self.send(&UserGetRequest).await
    }

    /// Update the user that owns the bearer token (`PUT /user`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when the client has no valid access token or the update is
    /// rejected.
    pub async fn update_user(&self, request: &UserUpdateRequest) -> Result<UserSchema, AuthError> {
        self.send(request).await
    }

    /// Revoke the session of the bearer token (`POST /logout`).
    ///
    /// # Errors
    /// Returns [`AuthError::Api`] when the client has no valid access token.
    pub async fn sign_out(&self) -> Result<(), AuthError> {
        self.send_without_body(&LogoutRequest::builder().build())
            .await
    }
}

/// Encapsulated HTTP request for the API
pub struct Request<T> {
    builder: reqwest::RequestBuilder,
    result: PhantomData<T>,
    retry_policy: rp_supabase_http::RetryPolicy,
}

impl<T> Request<T> {
    /// Execute an API request.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[instrument(name = "execute_request", skip(self), fields(method, path))]
    pub async fn execute(self) -> Result<Response<T>, AuthError> {
        let (client, request) = self.builder.build_split();
        let request = request?;

        // Record the method and path only. The query can contain tokens.
        let span = tracing::Span::current();
        span.record("method", request.method().as_str());
        span.record("path", request.url().path());

        // execute the request
        let response = rp_supabase_http::execute(&client, request, self.retry_policy).await?;

        Ok(Response {
            raw: response,
            result: PhantomData,
            span,
        })
    }
}

/// The raw response of the API request
pub struct Response<T> {
    raw: reqwest::Response,
    result: PhantomData<T>,
    // this span carries the context of the `Request`
    span: tracing::Span,
}

impl<T> Response<T> {
    /// Checks the HTTP status and discards a successful response body.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Api`] for a non-success status, or a transport error if the body
    /// cannot be read.
    #[instrument(name = "response_ok", skip(self), parent = &self.span)]
    pub async fn ok(self) -> Result<(), AuthError> {
        let status = self.raw.status();
        if status.is_success() {
            return Ok(());
        }
        let bytes = self.raw.bytes().await?.to_vec();
        Err(api_error(bytes, status))
    }

    /// Decodes a successful response body.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Api`] for a non-success status, or an error if reading or decoding
    /// the body fails.
    #[instrument(name = "parse_response_json", skip(self), parent = &self.span)]
    pub async fn json(self) -> Result<T, AuthError>
    where
        T: serde::de::DeserializeOwned,
    {
        let status = self.raw.status();
        let mut bytes = self.raw.bytes().await?.to_vec();
        if !status.is_success() {
            return Err(api_error(bytes, status));
        }
        tracing::debug!(status = %status, "Decoding response JSON");
        Ok(simd_json::from_slice::<T>(bytes.as_mut())?)
    }
}

fn api_error(mut bytes: Vec<u8>, status: reqwest::StatusCode) -> AuthError {
    let body = String::from_utf8_lossy(bytes.as_ref()).into_owned();
    tracing::debug!(status = %status, "Request returned an error status");

    let error = simd_json::from_slice::<ErrorSchema>(bytes.as_mut()).unwrap_or_else(|_| {
        let schema = ErrorSchema::builder().build();
        if body.is_empty() {
            schema
        } else {
            ErrorSchema {
                msg: Some(body),
                ..schema
            }
        }
    });
    AuthError::Api {
        status,
        error: Box::new(error),
    }
}

/// Builds the library's default transport. Redirects are followed only within the same origin.
#[expect(
    clippy::pub_with_shorthand,
    reason = "Rustfmt normalizes crate-private helper visibility to pub(crate)."
)]
pub(crate) fn default_client() -> Result<reqwest::Client, AuthError> {
    const KEEP_ALIVE_INTERVAL: core::time::Duration = core::time::Duration::from_secs(15);

    let temp_client = rp_supabase_http::client_builder()
        .http2_keep_alive_interval(KEEP_ALIVE_INTERVAL)
        .http2_keep_alive_while_idle(true)
        .build()?;
    Ok(temp_client)
}

fn base_headers(api_key: &str) -> Result<header::HeaderMap, AuthError> {
    let mut headers = header::HeaderMap::new();
    let mut key = header::HeaderValue::from_str(api_key)?;
    key.set_sensitive(true);
    headers.insert(SUPABASE_KEY, key);
    headers.insert(
        "Accept",
        header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        "Content-Type",
        header::HeaderValue::from_static("application/json"),
    );
    Ok(headers)
}
