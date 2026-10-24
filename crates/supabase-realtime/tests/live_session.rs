//! Existing anonymous/OAuth sessions must work without another password login.
#![expect(clippy::unwrap_used, reason = "Selected live acceptance assertions")]
#![expect(clippy::panic, reason = "Missing live prerequisites fail explicitly")]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Live integration test crate"
)]

use core::time::Duration;
use futures::StreamExt as _;
use rp_supabase_realtime::{
    message::ProtocolPayload,
    realtime::RealtimeConnection,
    rp_supabase_auth::{
        admin::AdminClient, auth_client::ApiClient, jwt_stream::SupabaseAuthConfig,
    },
};

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_error| panic!("live session acceptance requires {name}"))
}

#[derive(serde::Serialize)]
struct State {
    user_id: String,
}

#[tokio::test]
#[ignore = "requires local Supabase with anonymous sign-ins enabled"]
async fn anonymous_session_joins_and_tracks_presence_without_password() {
    let url = url::Url::parse(&required("SUPABASE_URL")).unwrap();
    let key = required("SUPABASE_ANON_KEY");
    let api = ApiClient::new_unauthenticated(&url, &key).unwrap();
    let session = api.sign_in_anonymously(None, None).await.unwrap();
    let id = session
        .user
        .as_ref()
        .and_then(|user| user.id.as_ref())
        .unwrap()
        .clone();
    let (mut stream, mut channel) = RealtimeConnection::presence(
        SupabaseAuthConfig {
            url: url.clone(),
            api_key: key,
            max_reconnect_attempts: 2,
            reconnect_interval: Duration::from_millis(100),
        },
        &format!("session-{}", std::process::id()),
    )
    .connect_with_session(session)
    .await
    .unwrap();
    channel.join(Some(id.clone())).await.unwrap();
    channel
        .track(&State {
            user_id: id.clone(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let message = stream.next().await.unwrap().unwrap();
            if matches!(&message.payload, ProtocolPayload::PresenceState(state) if state.0.contains_key(&id))
                || matches!(&message.payload, ProtocolPayload::PresenceDiff(diff) if diff.joins.contains_key(&id)) {
                break;
            }
        }
    }).await.unwrap();
    stream.shutdown().await;
    AdminClient::new(&url, &required("SUPABASE_SERVICE_ROLE_KEY"))
        .unwrap()
        .delete_user(&id, false)
        .await
        .unwrap();
}
