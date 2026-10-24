use crate::{
    auth_client::ApiClient,
    error::AuthError,
    session_store::{SessionStore, SessionStoreError, validate_session},
    types::{AccessTokenResponseSchema, LoginCredentials},
};
use alloc::sync::Arc;
use core::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::Stream;
use thiserror::Error;
use tokio::task::JoinSet;

#[derive(Clone, Debug, PartialEq, Eq, typed_builder::TypedBuilder)]
pub struct SupabaseAuthConfig {
    pub api_key: String,
    pub max_reconnect_attempts: u8,
    pub reconnect_interval: Duration,
    pub url: url::Url,
}

pub struct JwtStream {
    config: SupabaseAuthConfig,
    store: Option<Arc<dyn SessionStore>>,
}
impl JwtStream {
    #[must_use]
    pub const fn new(config: SupabaseAuthConfig) -> Self {
        Self {
            config,
            store: None,
        }
    }
    #[must_use]
    pub fn with_session_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }
    /// # Errors
    /// Returns transport or project configuration errors.
    pub fn sign_in(&self, params: LoginCredentials) -> Result<JwtRefreshStream, AuthError> {
        self.sign_in_with_client(params, crate::auth_client::default_client()?)
    }
    /// # Errors
    /// Returns project configuration errors.
    pub fn sign_in_with_client(
        &self,
        params: LoginCredentials,
        http: reqwest::Client,
    ) -> Result<JwtRefreshStream, AuthError> {
        let client = ApiClient::new_unauthenticated_with_client(
            &self.config.url,
            &self.config.api_key,
            http,
        )?;
        Ok(self.stream(client, Action::Login(params)))
    }
    /// Begin with a session from OAuth, anonymous sign-in or MFA; save before yielding.
    /// # Errors
    /// Returns invalid-session or client configuration errors.
    pub fn with_session(
        &self,
        session: AccessTokenResponseSchema,
    ) -> Result<JwtRefreshStream, AuthError> {
        self.with_session_with_client(session, crate::auth_client::default_client()?)
    }
    /// # Errors
    /// Returns invalid-session or project configuration errors.
    pub fn with_session_with_client(
        &self,
        session: AccessTokenResponseSchema,
        http: reqwest::Client,
    ) -> Result<JwtRefreshStream, AuthError> {
        validate_session(&session)?;
        let client = ApiClient::new_unauthenticated_with_client(
            &self.config.url,
            &self.config.api_key,
            http,
        )?;
        Ok(self.stream(client, Action::Initial(Box::new(session))))
    }
    /// Load and immediately rotate a stored refresh token. Never falls back to password login.
    /// # Errors
    /// Returns missing, corrupt or failed storage and configuration errors.
    pub async fn resume(&self) -> Result<JwtRefreshStream, AuthError> {
        self.resume_with_client(crate::auth_client::default_client()?)
            .await
    }
    /// # Errors
    /// Returns storage, session validation or project configuration errors.
    pub async fn resume_with_client(
        &self,
        http: reqwest::Client,
    ) -> Result<JwtRefreshStream, AuthError> {
        let store = self.store.as_ref().ok_or(SessionStoreError::Missing)?;
        let session = store.load().await?.ok_or(SessionStoreError::Missing)?;
        validate_session(&session)?;
        let client = ApiClient::new_unauthenticated_with_client(
            &self.config.url,
            &self.config.api_key,
            http,
        )?;
        Ok(self.stream(
            client,
            Action::Refresh(
                session
                    .refresh_token
                    .ok_or(AuthError::InvalidSession("missing refresh token"))?,
                tokio::time::Instant::now(),
            ),
        ))
    }
    fn stream(&self, client: ApiClient, action: Action) -> JwtRefreshStream {
        JwtRefreshStream {
            client,
            action: Some(action),
            tasks: JoinSet::new(),
            store: self.store.clone(),
            terminated: false,
            attempts: 0,
            max_attempts: usize::from(self.config.max_reconnect_attempts).max(1),
            reconnect_interval: self.config.reconnect_interval,
            next_attempt: tokio::time::Instant::now(),
        }
    }
}
#[derive(Clone)]
enum Action {
    Login(LoginCredentials),
    Initial(Box<AccessTokenResponseSchema>),
    Refresh(String, tokio::time::Instant),
}
#[expect(
    clippy::partial_pub_fields,
    reason = "The client remains public while refresh scheduling is private implementation state."
)]
pub struct JwtRefreshStream {
    pub client: ApiClient,
    action: Option<Action>,
    tasks: JoinSet<Result<(AccessTokenResponseSchema, tokio::time::Instant), RefreshStreamError>>,
    store: Option<Arc<dyn SessionStore>>,
    terminated: bool,
    attempts: usize,
    max_attempts: usize,
    reconnect_interval: Duration,
    next_attempt: tokio::time::Instant,
}
impl JwtRefreshStream {
    fn spawn(&mut self, action: Action) {
        let client = self.client.clone();
        let store = self.store.clone();
        let next_attempt = self.next_attempt;
        self.tasks.spawn(async move {
            tokio::time::sleep_until(next_attempt).await;
            let session = match action {
                Action::Login(credentials) => client.sign_in_with_password(&credentials).await?,
                Action::Initial(session) => *session,
                Action::Refresh(token, deadline) => {
                    tokio::time::sleep_until(deadline).await;
                    client.refresh_session(&token).await?
                }
            };
            validate_session(&session)?;
            // Anchor expiry to token receipt, not a later consumer poll or store write.
            let remaining = session.expires_at.map_or_else(
                || session.expires_in.unwrap_or(0),
                |expiry| expiry.saturating_sub(chrono::Utc::now().timestamp()),
            );
            let deadline = refresh_deadline(calculate_refresh_sleep_duration(
                u64::try_from(remaining).unwrap_or(0),
            ))?;
            if let Some(store) = store {
                store.save(&session).await.map_err(AuthError::from)?;
            }
            Ok((session, deadline))
        });
    }
}
impl Stream for JwtRefreshStream {
    type Item = Result<AccessTokenResponseSchema, RefreshStreamError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }
        if self.tasks.is_empty() {
            if let Some(action) = self.action.clone() {
                self.spawn(action);
            } else {
                self.terminated = true;
                return Poll::Ready(None);
            }
        }
        match self.tasks.poll_join_next(cx) {
            Poll::Ready(Some(Ok(result))) => {
                match &result {
                    Ok((session, deadline)) => {
                        self.attempts = 0;
                        self.action = session
                            .refresh_token
                            .clone()
                            .map(|token| Action::Refresh(token, *deadline));
                    }
                    Err(RefreshStreamError::AuthError(
                        AuthError::SessionStore(_) | AuthError::InvalidSession(_),
                    )) => {
                        self.terminated = true;
                    }
                    Err(_) => {
                        let deadline = match refresh_deadline(self.reconnect_interval) {
                            Ok(deadline) => deadline,
                            Err(error) => {
                                self.terminated = true;
                                return Poll::Ready(Some(Err(error.into())));
                            }
                        };
                        self.next_attempt = deadline;
                        self.attempts = self.attempts.saturating_add(1);
                        if self.attempts >= self.max_attempts {
                            self.terminated = true;
                        }
                        if let Some(Action::Refresh(_, next_refresh)) = self.action.as_mut() {
                            *next_refresh = deadline;
                        }
                    }
                }
                Poll::Ready(Some(result.map(|(session, _deadline)| session)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.terminated = true;
                Poll::Ready(Some(Err(RefreshStreamError::Task(error))))
            }
            Poll::Ready(None) => {
                self.terminated = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
fn refresh_deadline(delay: Duration) -> Result<tokio::time::Instant, AuthError> {
    tokio::time::Instant::now()
        .checked_add(delay)
        .ok_or(AuthError::InvalidSession(
            "duration exceeds the monotonic clock range",
        ))
}
const fn calculate_refresh_sleep_duration(expires_in: u64) -> Duration {
    let half_lifetime = match expires_in.checked_div(2) {
        Some(seconds) => seconds,
        None => 0,
    };
    Duration::from_secs(half_lifetime)
}
#[derive(Debug, Error)]
pub enum RefreshStreamError {
    #[error("Auth error: {0}")]
    AuthError(#[from] AuthError),
    #[error("Authentication task failed")]
    Task(#[source] tokio::task::JoinError),
}
#[derive(Debug, Error)]
pub enum SignInError {
    #[error(transparent)]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    UrlParseError(#[from] url::ParseError),
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "allow for tests")]
mod auth_tests {
    use core::time::Duration;

    use futures::StreamExt as _;
    use mockito::Matcher;
    use pretty_assertions::assert_eq;
    use rp_supabase_mock::{SupabaseMockServer, make_jwt};
    use rstest::rstest;
    use test_log::test;
    use tokio::time::timeout;

    use super::*;

    fn ms(ms: u32) -> Duration {
        Duration::from_millis(ms.into())
    }

    #[rstest]
    #[test(tokio::test)]
    #[timeout(ms(5_000))]
    async fn test_successful_password_login() {
        let access_token = make_jwt(Duration::from_secs(3600)).unwrap();
        let mut ms = SupabaseMockServer::new().await;
        ms.register_jwt_password(&access_token).unwrap();
        let config = SupabaseAuthConfig {
            url: ms.server_url().unwrap(),
            api_key: "api-key".to_owned(),
            max_reconnect_attempts: 1,
            reconnect_interval: Duration::from_secs(1),
        };
        let supabase_auth = JwtStream::new(config);
        let token_body =
            LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());

        let mut stream = supabase_auth.sign_in(token_body).unwrap();

        let response = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();

        assert!(response.is_ok());
        let auth_response = response.unwrap();
        assert_eq!(auth_response.access_token.unwrap(), access_token);
        assert_eq!(auth_response.refresh_token.unwrap(), "some-refresh-token");
        assert_eq!(
            auth_response.user.unwrap().email.unwrap(),
            "user@example.com"
        );
    }

    #[rstest]
    #[test(tokio::test)]
    #[timeout(ms(100))]
    async fn test_password_login_error() {
        let mut ms = SupabaseMockServer::new().await;
        let _m1 = ms
            .mockito_server
            .mock("POST", "/auth/v1/token")
            .match_query(Matcher::Regex("grant_type=password".to_owned()))
            .with_status(400)
            .create();

        let config = SupabaseAuthConfig {
            url: ms.server_url().unwrap(),
            api_key: "api-key".to_owned(),
            max_reconnect_attempts: 2,
            reconnect_interval: Duration::from_secs(1),
        };
        let supabase_auth = JwtStream::new(config);
        let token_body =
            LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());

        let mut stream = supabase_auth.sign_in(token_body).unwrap();

        let response = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();

        response.unwrap_err();
    }
    #[rstest]
    #[test(tokio::test)]
    #[timeout(ms(100))]
    async fn test_password_login_error_no_retries() {
        let mut ms = SupabaseMockServer::new().await;
        let _m1 = ms
            .mockito_server
            .mock("POST", "/auth/v1/token")
            .match_query(Matcher::Regex("grant_type=password".to_owned()))
            .with_status(400)
            .create();

        let config = SupabaseAuthConfig {
            url: ms.server_url().unwrap(),
            api_key: "api-key".to_owned(),
            max_reconnect_attempts: 1,
            reconnect_interval: Duration::from_secs(1),
        };
        let supabase_auth = JwtStream::new(config);
        let token_body =
            LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());

        let mut stream = supabase_auth.sign_in(token_body).unwrap();

        let response = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap();

        response.unwrap().unwrap_err();
        assert!(stream.next().await.is_none());
    }

    #[rstest]
    #[test(tokio::test)]
    #[timeout(ms(100))]
    async fn test_retry_on_login_error() {
        let mut ms = SupabaseMockServer::new().await;
        let _m1 = ms
            .mockito_server
            .mock("POST", "/auth/v1/token")
            .match_query(Matcher::Regex("grant_type=password".to_owned()))
            .with_status(500)
            .create();
        let config = SupabaseAuthConfig {
            url: ms.server_url().unwrap(),
            api_key: "api-key".to_owned(),
            max_reconnect_attempts: 2,
            reconnect_interval: Duration::from_millis(20),
        };
        let supabase_auth = JwtStream::new(config);
        let token_body =
            LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());

        let mut stream = supabase_auth.sign_in(token_body).unwrap();

        let response = stream.next().await.unwrap();
        response.unwrap_err();
        ms.register_jwt_password(&make_jwt(Duration::from_secs(3600)).unwrap())
            .unwrap();
        let response = timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .unwrap();

        assert!(response.is_ok());
        let auth_response = response.unwrap();
        assert_eq!(auth_response.refresh_token.unwrap(), "some-refresh-token");
        assert_eq!(
            auth_response.user.unwrap().email.unwrap(),
            "user@example.com"
        );
    }

    #[rstest]
    #[test_log::test(tokio::test)]
    #[timeout(ms(3_000))]
    async fn test_use_refresh_token_on_expiry() {
        // setup
        let mut ms = SupabaseMockServer::new().await;
        let first_access_token = make_jwt(Duration::from_millis(5)).unwrap();
        ms.register_jwt_password(&first_access_token).unwrap();

        let new_access_token = make_jwt(Duration::from_secs(3600)).unwrap();
        ms.register_jwt_refresh(&new_access_token).unwrap();
        let config = SupabaseAuthConfig {
            url: ms.server_url().unwrap(),
            api_key: "api-key".to_owned(),
            max_reconnect_attempts: 1,
            reconnect_interval: Duration::from_millis(20),
        };
        let supabase_auth = JwtStream::new(config);

        // action
        let token_body =
            LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());
        let mut stream = supabase_auth.sign_in(token_body).unwrap();

        // Get the initial token
        let response1 = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert!(response1.is_ok());
        let auth_response1 = response1.unwrap();
        assert_eq!(auth_response1.access_token.unwrap(), first_access_token);
        assert_eq!(
            auth_response1.user.unwrap().email.unwrap(),
            "user@example.com"
        );

        // Wait for token to expire and refresh
        let response2 = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert!(response2.is_ok());
        let auth_response2 = response2.unwrap();
        assert_eq!(auth_response2.access_token.unwrap(), new_access_token);
        assert_eq!(
            auth_response2.user.unwrap().email.unwrap(),
            "user@example.com"
        );
    }
}
