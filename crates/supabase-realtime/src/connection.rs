use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::SupabaseRealtimeError;

pub type WsSupabaseConnection = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub async fn connect(url: &url::Url) -> Result<WsSupabaseConnection, SupabaseRealtimeError> {
    let mut url = url.clone();
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        _ => return Err(SupabaseRealtimeError::MisconfiguredStreamURL),
    };
    url.set_scheme(scheme)
        .map_err(|()| SupabaseRealtimeError::MisconfiguredStreamURL)?;
    if scheme == "ws" {
        let (socket, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
        return Ok(socket);
    }
    #[cfg(any(feature = "rustls", feature = "native-tls"))]
    let (socket, _) =
        tokio_tungstenite::connect_async_tls_with_config(url.as_str(), None, false, connector()?)
            .await?;
    #[cfg(not(any(feature = "rustls", feature = "native-tls")))]
    let (socket, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
    Ok(socket)
}

#[cfg(feature = "rustls")]
fn connector() -> Result<Option<tokio_tungstenite::Connector>, SupabaseRealtimeError> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        roots
            .add(cert)
            .map_err(|_error| SupabaseRealtimeError::CannotSetNativeCertificate)?;
    }
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| alloc::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| tokio_tungstenite::tungstenite::Error::Tls(error.into()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Some(tokio_tungstenite::Connector::Rustls(
        alloc::sync::Arc::new(config),
    )))
}

#[cfg(all(not(feature = "rustls"), feature = "native-tls"))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "The rustls implementation can fail while loading roots"
)]
fn connector() -> Result<Option<tokio_tungstenite::Connector>, SupabaseRealtimeError> {
    Ok(None)
}
