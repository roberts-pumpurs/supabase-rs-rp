use core::time::Duration;

use examples::get_supabase_credentials;
use rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_auth::types::LoginCredentials;
use rp_supabase_realtime::futures::StreamExt as _;
use rp_supabase_realtime::message::broadcast::Broadcast;
use rp_supabase_realtime::message::phx_join;
use rp_supabase_realtime::realtime::RealtimeConnection;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .from_env()?
                .add_directive("rp_supabase_auth=info".to_owned().parse()?)
                .add_directive("rp_supabase_realtime=info".to_owned().parse()?)
                .add_directive("examples=info".to_owned().parse()?)
                .add_directive("broadcast_example=info".to_owned().parse()?),
        )
        .init();
    color_eyre::install()?;

    let credentials = get_supabase_credentials()?;

    let config = SupabaseAuthConfig {
        api_key: credentials.anon_key,
        max_reconnect_attempts: 5,
        reconnect_interval: Duration::from_secs(3),
        url: credentials.supabase_api_url,
    };
    let login_credentials = LoginCredentials::email(credentials.email, credentials.password);
    let (mut realtime, mut client) = RealtimeConnection::broadcast(config, "af")
        .connect(login_credentials)
        .await?;

    client
        .join(phx_join::BroadcastConfig {
            self_item: true,
            ack: true,
        })
        .await?;
    client
        .broadcast(Broadcast {
            r#type: "broadcast".to_owned(),
            event: "update".to_owned(),
            payload: simd_json::json!({"aaa": "bbbb"}),
        })
        .await?;
    tracing::info!("pooling realtime connection");
    while let Some(msg) = realtime.next().await {
        tracing::info!(?msg, "message");
    }
    tracing::error!("realtime connection exited");

    Err(eyre::eyre!("unexpected realtime exit"))
}
