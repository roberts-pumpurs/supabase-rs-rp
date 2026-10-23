use core::time::Duration;

use examples::get_supabase_credentials;
use rp_supabase_auth::futures::StreamExt as _;
use rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_auth::types::LoginCredentials;
use rp_supabase_client::new_authenticated;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .from_env()?
                .add_directive("rp_supabase_auth=info".to_owned().parse()?)
                .add_directive("client_example=debug".to_owned().parse()?),
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

    let mut client_stream = new_authenticated(config, login_credentials)?;

    while let Some(client) = client_stream.next().await {
        tracing::debug!(?client, "new client");

        let Ok((client, token_response)) = client else {
            continue;
        };
        tracing::info!(?token_response, "token response");
        let res = client
            .from("messages")
            .select("*")
            .fetch::<simd_json::OwnedValue>()
            .await?;

        tracing::info!(?res, "postgrest response");
    }
    tracing::error!("realtime connection exited");

    Err(eyre::eyre!("client stream exited"))
}
