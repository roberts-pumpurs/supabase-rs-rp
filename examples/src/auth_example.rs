use core::time::Duration;

use examples::get_supabase_credentials;
use rp_supabase_auth::auth_client::{ApiClient, new_authenticated_stream};
use rp_supabase_auth::futures::StreamExt as _;
use rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_auth::types::LoginCredentials;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .from_env()?
                .add_directive("rp_supabase_auth=debug".to_owned().parse()?)
                .add_directive("auth_example=debug".to_owned().parse()?),
        )
        .init();

    let credentials = get_supabase_credentials()?;

    let config = SupabaseAuthConfig {
        api_key: credentials.anon_key,
        max_reconnect_attempts: 5,
        reconnect_interval: Duration::from_secs(3),
        url: credentials.supabase_api_url,
    };
    let login_credentials = LoginCredentials::email(credentials.email, credentials.password);

    // One-off session: sign in, read the user, sign out.
    let anonymous = ApiClient::new_unauthenticated(&config.url, &config.api_key)?;
    let session = anonymous.sign_in_with_password(&login_credentials).await?;
    let access_token = session
        .access_token
        .ok_or_else(|| eyre::eyre!("sign-in returned no access token"))?;
    let client = ApiClient::new_authenticated(&config.url, &config.api_key, &access_token)?;
    let user = client.get_user().await?;
    tracing::info!(email = ?user.email, "signed in");
    client.sign_out().await?;

    // Long-lived session: the stream yields a new client after every token refresh.
    let mut auth_client_stream = new_authenticated_stream(config, login_credentials)?;
    while let Some(item) = auth_client_stream.next().await {
        let client = match item {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "token refresh failed");
                continue;
            }
        };
        let user = client.get_user().await?;
        tracing::info!(data = ?user, "user info");
    }
    tracing::error!("auth stream exited");

    Err(eyre::eyre!("unexpected exit"))
}
