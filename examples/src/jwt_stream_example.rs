use core::time::Duration;

use color_eyre::eyre;
use examples::get_supabase_credentials;
use rp_supabase_auth::futures::StreamExt as _;
use rp_supabase_auth::jwt_stream::{JwtStream, SupabaseAuthConfig};
use rp_supabase_auth::types::LoginCredentials;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .from_env()?
                .add_directive("rp_supabase_auth=debug".to_owned().parse()?)
                .add_directive("jwt_stream_example=debug".to_owned().parse()?),
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
    let supabase_auth = JwtStream::new(config);
    let mut token_refresh = supabase_auth.sign_in(LoginCredentials::email(
        credentials.email,
        credentials.password,
    ))?;

    while let Some(msg) = token_refresh.next().await {
        tracing::debug!(?msg, "reading protocol message");
    }
    tracing::error!("realtime connection exited");

    Err(eyre::eyre!("unexpected exit"))
}
