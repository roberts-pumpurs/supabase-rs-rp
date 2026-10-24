//! Opt-in local `GoTrue` acceptance: `SUPABASE_URL` and `SUPABASE_ANON_KEY` are required.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "explicit live acceptance setup and assertions"
)]
#![expect(clippy::tests_outside_test_module, reason = "Live integration surface")]
extern crate alloc;

use alloc::sync::Arc;
use core::time::Duration;
use futures::StreamExt as _;
use rp_supabase_auth::{
    auth_client::ApiClient,
    jwt_stream::{JwtStream, SupabaseAuthConfig},
    session_store::{MemorySessionStore, SessionStore as _},
};
#[tokio::test]
#[ignore = "requires local Supabase with anonymous sign-ins enabled"]
async fn anonymous_persist_resume_rotate_and_clear() {
    let url = std::env::var("SUPABASE_URL")
        .expect("SUPABASE_URL required")
        .parse()
        .unwrap();
    let api_key = std::env::var("SUPABASE_ANON_KEY").expect("SUPABASE_ANON_KEY required");
    let client = ApiClient::new_unauthenticated(&url, &api_key).unwrap();
    let session = client.sign_in_anonymously(None, None).await.unwrap();
    assert_eq!(session.user.as_ref().unwrap().is_anonymous, Some(true));
    let store = Arc::new(MemorySessionStore::default());
    let factory = JwtStream::new(SupabaseAuthConfig {
        url,
        api_key,
        max_reconnect_attempts: 1,
        reconnect_interval: Duration::from_secs(1),
    })
    .with_session_store(Arc::<MemorySessionStore>::clone(&store));
    let mut initial = factory.with_session(session).unwrap();
    initial.next().await.unwrap().unwrap();
    drop(initial);
    let mut resumed = factory.resume().await.unwrap();
    let rotated = resumed.next().await.unwrap().unwrap();
    assert_eq!(
        store.load().await.unwrap().unwrap().refresh_token,
        rotated.refresh_token
    );
    drop(resumed);
    store.clear().await.unwrap();
    assert!(matches!(
        factory.resume().await,
        Err(rp_supabase_auth::error::AuthError::SessionStore(
            rp_supabase_auth::session_store::SessionStoreError::Missing
        ))
    ));
}
