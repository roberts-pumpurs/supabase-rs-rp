use core::time::Duration;

use examples::get_supabase_credentials;
use rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_auth::types::LoginCredentials;
use rp_supabase_realtime::futures::StreamExt as _;
use rp_supabase_realtime::message::phx_join::PostgresChanges;
use rp_supabase_realtime::realtime::{RealtimeConnection, typed_changes};
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
                .add_directive("db_updates_example=info".to_owned().parse()?),
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
    let (realtime, mut client) = RealtimeConnection::db_changes(config)
        .connect(login_credentials)
        .await?;

    client
        .subscribe_to_changes(vec![PostgresChanges::table("messages")])
        .await?;
    tracing::info!("polling realtime connection");
    let mut changes = core::pin::pin!(typed_changes::<simd_json::OwnedValue>(realtime));
    while let Some(change) = changes.next().await {
        match change {
            Ok(change) => tracing::info!(?change, "postgres change"),
            Err(err) => tracing::warn!(?err, "realtime error"),
        }
    }
    tracing::error!("realtime connection exited");

    Err(eyre::eyre!("should not have exited"))
}
