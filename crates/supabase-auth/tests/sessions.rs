#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Public session integration surface"
)]
extern crate alloc;

use alloc::sync::Arc;
use core::time::Duration;
use futures::StreamExt as _;
use rp_supabase_auth::{
    auth_client::{ApiClient, requests::AuthorizeRequest},
    jwt_stream::{JwtStream, SupabaseAuthConfig},
    session_store::{MemorySessionStore, SessionStore, SessionStoreError, StoreFuture},
    types::{AccessTokenResponseSchema, OAuthProvider},
};
fn session(token: &str) -> AccessTokenResponseSchema {
    AccessTokenResponseSchema::builder()
        .access_token(token.to_owned())
        .refresh_token(format!("refresh-{token}"))
        .expires_in(3600)
        .build()
}
fn config(url: url::Url) -> SupabaseAuthConfig {
    SupabaseAuthConfig {
        url,
        api_key: "key".into(),
        max_reconnect_attempts: 1,
        reconnect_interval: Duration::ZERO,
    }
}
#[tokio::test]
async fn initial_save_clear_and_redaction() {
    let store = Arc::new(MemorySessionStore::default());
    let factory = JwtStream::new(config(url::Url::parse("http://localhost").unwrap()))
        .with_session_store(Arc::<MemorySessionStore>::clone(&store));
    let mut stream = factory.with_session(session("secret-token")).unwrap();
    let result = stream.next().await.unwrap().unwrap();
    assert_eq!(
        store.load().await.unwrap().unwrap().access_token,
        result.access_token
    );
    assert!(!format!("{result:?} {store:?}").contains("secret-token"));
    store.clear().await.unwrap();
    assert!(store.load().await.unwrap().is_none());
    assert!(matches!(
        factory.resume().await,
        Err(rp_supabase_auth::error::AuthError::SessionStore(
            SessionStoreError::Missing
        ))
    ));
}
struct Broken;
impl SessionStore for Broken {
    fn load(&self) -> StoreFuture<'_, Option<AccessTokenResponseSchema>> {
        Box::pin(async { Err(SessionStoreError::Corrupt) })
    }
    fn save<'a>(&'a self, _: &'a AccessTokenResponseSchema) -> StoreFuture<'a, ()> {
        Box::pin(async { Err(SessionStoreError::Corrupt) })
    }
    fn clear(&self) -> StoreFuture<'_, ()> {
        Box::pin(async { Err(SessionStoreError::Corrupt) })
    }
}
#[tokio::test]
async fn save_failure_is_yielded_once_then_terminates() {
    let factory = JwtStream::new(config(url::Url::parse("http://localhost").unwrap()))
        .with_session_store(Arc::new(Broken));
    let mut stream = factory.with_session(session("secret")).unwrap();
    stream.next().await.unwrap().unwrap_err();
    assert!(stream.next().await.is_none());
}
#[tokio::test]
async fn resume_rotates_only_refresh_token_and_saves_before_yield() {
    let mut server = mockito::Server::new_async().await;
    let refresh = server
        .mock("POST", "/auth/v1/token")
        .match_query(mockito::Matcher::UrlEncoded(
            "grant_type".into(),
            "refresh_token".into(),
        ))
        .match_body(mockito::Matcher::JsonString(
            r#"{"refresh_token":"refresh-old"}"#.into(),
        ))
        .with_status(200)
        .with_body(r#"{"access_token":"new","refresh_token":"rotated","expires_in":3600}"#)
        .expect(1)
        .create_async()
        .await;
    let store = Arc::new(MemorySessionStore::default());
    store.save(&session("old")).await.unwrap();
    let factory = JwtStream::new(config(server.url().parse().unwrap())).with_session_store(Arc::<
        MemorySessionStore,
    >::clone(
        &store,
    ));
    let mut stream = factory.resume().await.unwrap();
    assert_eq!(
        stream
            .next()
            .await
            .unwrap()
            .unwrap()
            .access_token
            .as_deref(),
        Some("new")
    );
    assert_eq!(
        store
            .load()
            .await
            .unwrap()
            .unwrap()
            .refresh_token
            .as_deref(),
        Some("rotated")
    );
    refresh.assert_async().await;
}

#[tokio::test(start_paused = true)]
async fn password_retry_does_not_run_before_reconnect_interval() {
    let mut server = mockito::Server::new_async().await;
    let failure = server
        .mock("POST", "/auth/v1/token")
        .match_query(mockito::Matcher::UrlEncoded(
            "grant_type".into(),
            "password".into(),
        ))
        .with_status(503)
        .with_body(r#"{"message":"temporarily unavailable"}"#)
        .expect(2)
        .create_async()
        .await;
    let mut settings = config(server.url().parse().unwrap());
    settings.max_reconnect_attempts = 2;
    settings.reconnect_interval = Duration::from_secs(60);
    let mut stream = JwtStream::new(settings)
        .sign_in(rp_supabase_auth::types::LoginCredentials::email(
            "user@example.com".into(),
            "password".into(),
        ))
        .unwrap();
    stream.next().await.unwrap().unwrap_err();
    tokio::time::advance(Duration::from_secs(40)).await;
    {
        let next = stream.next();
        futures::pin_mut!(next);
        assert!(futures::poll!(next.as_mut()).is_pending());
        tokio::task::yield_now().await;
        assert!(futures::poll!(next.as_mut()).is_pending());
    }
    tokio::time::advance(Duration::from_secs(20)).await;
    stream.next().await.unwrap().unwrap_err();
    assert!(stream.next().await.is_none());
    failure.assert_async().await;
}

#[expect(
    clippy::expect_used,
    reason = "The timeout assertion describes the refresh scheduling regression"
)]
#[tokio::test(start_paused = true)]
async fn regression_delayed_poll_does_not_restart_refresh_half_life() {
    let mut server = mockito::Server::new_async().await;
    let refresh = server
        .mock("POST", "/auth/v1/token")
        .match_query(mockito::Matcher::UrlEncoded(
            "grant_type".into(),
            "refresh_token".into(),
        ))
        .match_body(mockito::Matcher::JsonString(
            r#"{"refresh_token":"refresh-old"}"#.into(),
        ))
        .with_status(200)
        .with_body(r#"{"access_token":"new","refresh_token":"rotated","expires_in":3600}"#)
        .expect(1)
        .create_async()
        .await;
    let factory = JwtStream::new(config(server.url().parse().unwrap()));
    let mut stream = factory.with_session(session("old")).unwrap();
    assert_eq!(
        stream
            .next()
            .await
            .unwrap()
            .unwrap()
            .access_token
            .as_deref(),
        Some("old")
    );

    // No consumer polls the stream while the scheduled half-life passes.
    tokio::time::advance(Duration::from_secs(1801)).await;
    // Use real time for HTTP I/O: paused-time auto-advance can race socket readiness.
    tokio::time::resume();
    let refreshed = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("refresh is already due; polling must not start another 1800-second wait")
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.access_token.as_deref(), Some("new"));
    assert_eq!(refreshed.refresh_token.as_deref(), Some("rotated"));
    refresh.assert_async().await;
}
#[test]
fn oauth_encoding_and_randomness() {
    let client =
        ApiClient::new_unauthenticated(&"http://localhost".parse().unwrap(), "key").unwrap();
    let flow = client
        .sign_in_with_oauth(
            AuthorizeRequest::builder()
                .provider(OAuthProvider::GitHub)
                .redirect_to("http://localhost/callback?next=a%26b".parse().unwrap())
                .scopes(vec!["user:email".into(), "a+b".into()])
                .build(),
        )
        .unwrap();
    let query: std::collections::HashMap<_, _> = flow.authorization_url.query_pairs().collect();
    assert_eq!(
        query.get("scopes").map(alloc::borrow::Cow::as_ref),
        Some("user:email a+b")
    );
    assert_eq!(
        query.get("redirect_to").map(alloc::borrow::Cow::as_ref),
        Some("http://localhost/callback?next=a%26b")
    );
    assert_eq!(
        query.get("code_challenge").map(alloc::borrow::Cow::as_ref),
        Some(flow.verifier.challenge().code_challenge.as_str())
    );
    assert_eq!(flow.verifier.as_str().len(), 43);
    assert!(!format!("{flow:?}").contains(flow.verifier.as_str()));
}
#[tokio::test]
async fn anonymous_validates_user_and_maps_api_error() {
    let mut server = mockito::Server::new_async().await;
    let success = server.mock("POST", "/auth/v1/signup").with_status(200).with_body(r#"{"access_token":"anon","refresh_token":"refresh","expires_in":3600,"user":{"is_anonymous":true}}"#).create_async().await;
    let client = ApiClient::new_unauthenticated(&server.url().parse().unwrap(), "key").unwrap();
    assert_eq!(
        client
            .sign_in_anonymously(None, None)
            .await
            .unwrap()
            .user
            .unwrap()
            .is_anonymous,
        Some(true)
    );
    success.remove_async().await;
    let _error = server
        .mock("POST", "/auth/v1/signup")
        .with_status(422)
        .with_body(r#"{"msg":"Anonymous sign-ins disabled"}"#)
        .create_async()
        .await;
    assert!(
        matches!(client.sign_in_anonymously(None, None).await, Err(rp_supabase_auth::error::AuthError::Api { status, .. }) if status.as_u16() == 422)
    );
}
#[cfg(feature = "file-session-store")]
#[tokio::test]
async fn file_corruption_permissions_and_symlink_replacement() {
    use rp_supabase_auth::session_store::FileSessionStore;
    let path = std::env::temp_dir().join(format!(
        "supabase-session-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let store = FileSessionStore::new(&path);
    assert!(store.load().await.unwrap().is_none());
    store.save(&session("file-secret")).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            tokio::fs::metadata(&path)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let target = path.with_extension("target");
        tokio::fs::write(&target, b"untouched").await.unwrap();
        store.clear().await.unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        store.save(&session("replacement")).await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"untouched");
        tokio::fs::remove_file(target).await.unwrap();
    };
    tokio::fs::write(&path, b"invalid-json-secret")
        .await
        .unwrap();
    assert!(matches!(
        store.load().await,
        Err(SessionStoreError::Corrupt)
    ));
    store.clear().await.unwrap();
    store.clear().await.unwrap();
}
