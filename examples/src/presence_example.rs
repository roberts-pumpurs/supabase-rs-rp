use core::time::Duration;

use examples::get_supabase_credentials;
use rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_auth::types::LoginCredentials;
use rp_supabase_realtime::futures::StreamExt as _;
use rp_supabase_realtime::realtime::RealtimeConnection;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Deserialize, Serialize)]
struct MessageToTrack {
    example_field_b: String,
    field_a: String,
    random_number: u64,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .from_env()?
                .add_directive("rp_supabase_auth=info".to_owned().parse()?)
                .add_directive("rp_supabase_realtime=info".to_owned().parse()?)
                .add_directive("examples=info".to_owned().parse()?)
                .add_directive("presence_example=info".to_owned().parse()?),
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
    let (mut realtime, mut client) = RealtimeConnection::presence(config, "af")
        .connect_with_state_tracking::<simd_json::OwnedValue>(login_credentials)
        .await?;

    client.join(None).await?;
    client
        .track(&MessageToTrack {
            field_a: "todo!()".to_owned(),
            example_field_b: "data b inner".to_owned(),
            random_number: 123,
        })
        .await?;
    tracing::info!("pooling realtime connection");
    while let Some(msg) = realtime.next().await {
        tracing::info!(?msg, "message");
    }
    tracing::error!("realtime connection exited");

    Err(eyre::eyre!("unexpected realtime exit"))
}
