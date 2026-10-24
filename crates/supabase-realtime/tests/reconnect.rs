#![expect(
    clippy::tests_outside_test_module,
    reason = "Standalone integration test crate"
)]
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Loopback regression fixtures fail immediately with explicit protocol and deadline assertions"
)]
#[cfg(any(feature = "rustls", feature = "native-tls"))]
extern crate alloc;

use core::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use rp_supabase_realtime::{
    ConnectionOptions, ConnectionStatus, message::ProtocolMessage, realtime::RealtimeBaseConnection,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

type Peer = WebSocketStream<TcpStream>;

fn decode(text: &str) -> ProtocolMessage {
    simd_json::from_slice(&mut text.as_bytes().to_vec()).unwrap()
}

fn join() -> ProtocolMessage {
    decode(
        r#"{"topic":"realtime:room","event":"phx_join","payload":{"config":{"private":true,"broadcast":{"self":true,"ack":true},"presence":{"enabled":true,"key":"alice"},"postgres_changes":[]},"access_token":"old"},"ref":null,"join_ref":null}"#,
    )
}

async fn read(peer: &mut Peer) -> ProtocolMessage {
    loop {
        let frame = peer.next().await.unwrap().unwrap();
        if let Message::Text(text) = frame {
            return decode(&text);
        }
    }
}

async fn ack(peer: &mut Peer, message: &ProtocolMessage, status: &str) {
    let text = format!(
        r#"{{"topic":"{}","event":"phx_reply","payload":{{"status":"{status}","response":{{"reason":"denied"}}}},"ref":"{}","join_ref":"{}"}}"#,
        message.topic,
        message.ref_field.as_ref().unwrap(),
        message.join_ref.as_deref().unwrap_or("")
    );
    peer.send(Message::Text(text.into())).await.unwrap();
}

fn options() -> ConnectionOptions {
    ConnectionOptions {
        initial_backoff: Duration::from_millis(40),
        max_backoff: Duration::from_millis(40),
        heartbeat_interval: Duration::from_secs(60),
        acknowledgement_timeout: Duration::from_secs(2),
        ..ConnectionOptions::default()
    }
}

async fn setup(
    options: ConnectionOptions,
) -> (
    TcpListener,
    rp_supabase_realtime::RealtimeStream,
    futures::channel::mpsc::Sender<
        Result<ProtocolMessage, rp_supabase_realtime::error::SupabaseRealtimeError>,
    >,
    Peer,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = url::Url::parse(&format!(
        "ws://{}/realtime/v1/websocket",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let (tx, rx) = futures::channel::mpsc::channel(2);
    let (stream, peer) = tokio::try_join!(
        RealtimeBaseConnection::new(url)
            .with_options(options)
            .connect(rx),
        async {
            let (socket, _) = listener.accept().await?;
            Ok::<_, rp_supabase_realtime::error::SupabaseRealtimeError>(
                tokio_tungstenite::accept_async(socket).await?,
            )
        }
    )
    .unwrap();
    (listener, stream, tx, peer)
}

#[tokio::test]
async fn reconnect_replays_latest_join_and_token_not_broadcasts() {
    let (listener, mut stream, mut tx, mut peer) = setup(options()).await;
    tx.send(Ok(join())).await.unwrap();
    let first = read(&mut peer).await;
    ack(&mut peer, &first, "ok").await;
    stream.next().await.unwrap().unwrap();
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Ready);
    tx.send(Ok(decode(r#"{"topic":"realtime:room","event":"access_token","payload":{"access_token":"new"},"ref":null,"join_ref":null}"#))).await.unwrap();
    let _token = read(&mut peer).await;
    tx.send(Ok(decode(r#"{"topic":"realtime:room","event":"broadcast","payload":{"type":"broadcast","event":"once","payload":{}},"ref":null,"join_ref":null}"#))).await.unwrap();
    let broadcast = read(&mut peer).await;
    assert!(matches!(
        broadcast.payload,
        rp_supabase_realtime::message::ProtocolPayload::Broadcast(_)
    ));
    let mut status = stream.status();
    drop(peer);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            status.changed().await.unwrap();
            if matches!(*status.borrow(), ConnectionStatus::Reconnecting { .. }) {
                break;
            }
        }
    })
    .await
    .unwrap();
    tx.send(Ok(decode(r#"{"topic":"realtime:room","event":"access_token","payload":{"access_token":"new-during-backoff"},"ref":null,"join_ref":null}"#))).await.unwrap();
    let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut peer = tokio_tungstenite::accept_async(socket).await.unwrap();
    let replay = read(&mut peer).await;
    assert_ne!(first.ref_field, replay.ref_field);
    assert_ne!(first.join_ref, replay.join_ref);
    let rp_supabase_realtime::message::ProtocolPayload::PhxJoin(join) = replay.payload else {
        panic!("expected replayed join")
    };
    assert!(join.config.private);
    assert_eq!(join.config.presence.key, "alice");
    assert_eq!(join.access_token.as_deref(), Some("new-during-backoff"));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.next())
            .await
            .is_err(),
        "historical broadcasts must not replay"
    );
    stream.shutdown().await;
}

#[tokio::test]
async fn rejected_join_is_never_ready() {
    let (_listener, mut stream, mut tx, mut peer) = setup(options()).await;
    tx.send(Ok(join())).await.unwrap();
    let message = read(&mut peer).await;
    ack(&mut peer, &message, "error").await;
    assert!(matches!(
        stream.next().await.unwrap(),
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::ChannelError { .. })
    ));
    assert_ne!(*stream.status().borrow(), ConnectionStatus::Ready);
}

#[tokio::test]
async fn wrong_heartbeat_reference_times_out_and_exhausts_budget() {
    let options = ConnectionOptions {
        max_retries: 0,
        heartbeat_interval: Duration::from_millis(40),
        acknowledgement_timeout: Duration::from_millis(150),
        ..options()
    };
    let (_listener, mut stream, _tx, mut peer) = setup(options).await;
    let heartbeat = read(&mut peer).await;
    assert_eq!(heartbeat.topic, "phoenix");
    peer.send(Message::Text(r#"{"topic":"phoenix","event":"phx_reply","payload":{"status":"ok","response":{}},"ref":"stale","join_ref":null}"#.into())).await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        error,
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::HeartbeatTimeout)
    ));
    assert!(matches!(
        stream.next().await.unwrap(),
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::ReconnectExhausted)
    ));
}

#[tokio::test]
async fn shutdown_cancels_backoff_and_drop_cancels_socket() {
    let (listener, mut stream, _tx, peer) = setup(ConnectionOptions {
        initial_backoff: Duration::from_secs(30),
        max_backoff: Duration::from_secs(30),
        ..options()
    })
    .await;
    let mut status = stream.status();
    drop(peer);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            status.changed().await.unwrap();
            if matches!(*status.borrow(), ConnectionStatus::Reconnecting { .. }) {
                break;
            }
        }
    })
    .await
    .unwrap();
    stream.shutdown().await;
    tokio::time::timeout(Duration::from_millis(100), listener.accept())
        .await
        .expect_err("shutdown must prevent reconnect");
}

#[tokio::test]
async fn ws_ip_uses_plain_tcp() {
    let (_listener, stream, _tx, mut peer) = setup(options()).await;
    drop(stream);
    let closed = tokio::time::timeout(Duration::from_secs(1), peer.next())
        .await
        .unwrap();
    assert!(
        closed.is_none_or(|message| message.is_err() || matches!(message, Ok(Message::Close(_))))
    );
}

#[cfg(any(feature = "rustls", feature = "native-tls"))]
#[tokio::test]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Any other present or future connection error must fail the certificate check"
)]
async fn wss_ip_rejects_untrusted_certificate() {
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der());
    let config = rustls::ServerConfig::builder_with_provider(alloc::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![certificate.cert.der().clone()], key.into())
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(alloc::sync::Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = url::Url::parse(&format!(
        "wss://{}/websocket",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let (_tx, rx) = futures::channel::mpsc::channel(1);
    let (client, _server) = tokio::join!(RealtimeBaseConnection::new(url).connect(rx), async {
        let (socket, _) = listener.accept().await.unwrap();
        acceptor.accept(socket).await
    });
    let error = client
        .err()
        .expect("untrusted IP certificate must not be accepted");
    match error {
        rp_supabase_realtime::error::SupabaseRealtimeError::Transport(
            tokio_tungstenite::tungstenite::Error::Tls(_),
        ) => {}
        rp_supabase_realtime::error::SupabaseRealtimeError::Transport(
            tokio_tungstenite::tungstenite::Error::Io(error),
        ) => {
            let certificate_error = error
                .get_ref()
                .and_then(|cause| cause.downcast_ref::<rustls::Error>());
            assert!(
                matches!(
                    certificate_error,
                    Some(rustls::Error::InvalidCertificate(
                        rustls::CertificateError::UnknownIssuer
                    ))
                ),
                "expected an unknown certificate issuer, got {error:?}"
            );
        }
        other => panic!("expected certificate verification failure, got {other:?}"),
    }
}

#[tokio::test]
async fn bounded_command_queue_reports_full_without_discarding() {
    let (mut tx, mut rx) = futures::channel::mpsc::channel(1);
    tx.try_send(Ok::<_, rp_supabase_realtime::error::SupabaseRealtimeError>(
        join(),
    ))
    .unwrap();
    // futures mpsc reserves one slot for each sender, in addition to capacity.
    tx.try_send(Ok(join())).unwrap();
    let error = tx.try_send(Ok(join())).unwrap_err();
    assert!(error.is_full());
    assert_eq!(rx.next().await.unwrap().unwrap().topic, "realtime:room");
    assert_eq!(rx.next().await.unwrap().unwrap().topic, "realtime:room");
}

#[tokio::test]
async fn replay_retains_database_filters_and_ignores_old_join_acknowledgement() {
    let (listener, mut stream, mut tx, mut peer) = setup(ConnectionOptions {
        heartbeat_interval: Duration::from_secs(60),
        ..options()
    })
    .await;
    let mut intended = join();
    let rp_supabase_realtime::message::ProtocolPayload::PhxJoin(config) = &mut intended.payload
    else {
        panic!("join")
    };
    config
        .config
        .postgres_changes
        .push(rp_supabase_realtime::message::phx_join::PostgresChanges {
            event: rp_supabase_realtime::message::phx_join::PostgresChangeEvent::Insert,
            schema: "public".to_owned(),
            table: "product_events".to_owned(),
            filter: Some("owner=eq.alice".to_owned()),
        });
    tx.send(Ok(intended)).await.unwrap();
    let first = read(&mut peer).await;
    ack(&mut peer, &first, "ok").await;
    stream.next().await.unwrap().unwrap();
    assert_ne!(*stream.status().borrow(), ConnectionStatus::Ready);
    drop(peer);
    let (socket, _) = listener.accept().await.unwrap();
    let mut peer = tokio_tungstenite::accept_async(socket).await.unwrap();
    let replay = read(&mut peer).await;
    let rp_supabase_realtime::message::ProtocolPayload::PhxJoin(config) = &replay.payload else {
        panic!("join replay")
    };
    assert_eq!(
        config
            .config
            .postgres_changes
            .first()
            .unwrap()
            .filter
            .as_deref(),
        Some("owner=eq.alice")
    );
    ack(&mut peer, &first, "ok").await;
    peer.send(Message::Text(format!(r#"{{"topic":"realtime:room","event":"system","payload":{{"status":"ok","message":"subscribed","extension":"postgres_changes","channel":"room"}},"ref":null,"join_ref":"{}"}}"#, replay.join_ref.as_ref().unwrap()).into())).await.unwrap();
    loop {
        if let Some(Ok(message)) = stream.next().await
            && matches!(
                message.payload,
                rp_supabase_realtime::message::ProtocolPayload::System(_)
            )
        {
            break;
        }
    }
    assert_ne!(*stream.status().borrow(), ConnectionStatus::Ready);
    ack(&mut peer, &replay, "ok").await;
    stream.next().await.unwrap().unwrap();
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Ready);
    stream.shutdown().await;
}

#[tokio::test]
async fn presence_retracks_only_latest_payload_after_rejoin() {
    let (listener, mut stream, mut tx, mut peer) = setup(ConnectionOptions {
        heartbeat_interval: Duration::from_secs(60),
        ..options()
    })
    .await;
    tx.send(Ok(join())).await.unwrap();
    let first = read(&mut peer).await;
    ack(&mut peer, &first, "ok").await;
    stream.next().await.unwrap().unwrap();
    for value in [1_u8, 2] {
        tx.send(Ok(decode(&format!(r#"{{"topic":"realtime:room","event":"presence","payload":{{"type":"presence","event":"track","payload":{{"value":{value}}}}},"ref":null,"join_ref":null}}"#)))).await.unwrap();
        let _track = read(&mut peer).await;
    }
    drop(peer);
    let (socket, _) = listener.accept().await.unwrap();
    let mut peer = tokio_tungstenite::accept_async(socket).await.unwrap();
    let replay = read(&mut peer).await;
    ack(&mut peer, &replay, "ok").await;
    let track = read(&mut peer).await;
    let encoded = String::from_utf8(simd_json::to_vec(&track).unwrap()).unwrap();
    assert!(encoded.contains(r#""value":2"#));
    assert_eq!(track.join_ref, replay.join_ref);
    stream.shutdown().await;
}

#[tokio::test]
async fn terminal_authentication_error_is_visible_and_stops_reconnect() {
    let (_listener, mut stream, mut tx, _peer) = setup(options()).await;
    tx.send(Err(
        rp_supabase_realtime::error::SupabaseRealtimeError::JwtStreamClosedUnexpectedly,
    ))
    .await
    .unwrap();
    assert!(matches!(
        stream.next().await.unwrap(),
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::JwtStreamClosedUnexpectedly)
    ));
    assert!(stream.next().await.is_none());
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Failed);
}

#[tokio::test]
async fn reconnect_failures_exhaust_configured_attempts() {
    let (listener, mut stream, _tx, peer) = setup(ConnectionOptions {
        max_retries: 2,
        ..options()
    })
    .await;
    drop(listener);
    drop(peer);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match stream.next().await {
                Some(Err(
                    rp_supabase_realtime::error::SupabaseRealtimeError::ReconnectExhausted,
                )) => break,
                Some(_) => {}
                None => panic!("driver closed without exposing retry exhaustion"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Failed);
}

async fn regression_next(
    stream: &mut rp_supabase_realtime::RealtimeStream,
) -> Result<ProtocolMessage, rp_supabase_realtime::error::SupabaseRealtimeError> {
    tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("driver must make progress within its configured deadlines")
        .expect("driver must not terminate before reporting the expected event")
}

async fn regression_join(
    stream: &mut rp_supabase_realtime::RealtimeStream,
    tx: &mut futures::channel::mpsc::Sender<
        Result<ProtocolMessage, rp_supabase_realtime::error::SupabaseRealtimeError>,
    >,
    peer: &mut Peer,
) -> ProtocolMessage {
    tx.send(Ok(join())).await.unwrap();
    let joined = read(peer).await;
    ack(peer, &joined, "ok").await;
    regression_next(stream).await.unwrap();
    joined
}

#[tokio::test]
async fn regression_current_generation_broadcast_ack_is_forwarded() {
    let (_listener, mut stream, mut tx, mut peer) = setup(options()).await;
    let joined = regression_join(&mut stream, &mut tx, &mut peer).await;
    tx.send(Ok(decode(r#"{"topic":"realtime:room","event":"broadcast","payload":{"type":"broadcast","event":"once","payload":{"value":7}},"ref":null,"join_ref":null}"#))).await.unwrap();
    let push = read(&mut peer).await;
    assert_eq!(push.join_ref, joined.join_ref);
    assert_ne!(push.ref_field, joined.ref_field);
    ack(&mut peer, &push, "ok").await;
    let reply = regression_next(&mut stream).await.unwrap();
    assert_eq!(reply.ref_field, push.ref_field);
    assert_eq!(reply.join_ref, joined.join_ref);
    assert!(matches!(
        reply.payload,
        rp_supabase_realtime::message::ProtocolPayload::PhxReply(
            rp_supabase_realtime::message::phx_reply::PhxReply::Ok(_)
        )
    ));
    stream.shutdown().await;
}

#[tokio::test]
async fn regression_current_generation_presence_error_is_forwarded() {
    let (_listener, mut stream, mut tx, mut peer) = setup(options()).await;
    let joined = regression_join(&mut stream, &mut tx, &mut peer).await;
    tx.send(Ok(decode(r#"{"topic":"realtime:room","event":"presence","payload":{"type":"presence","event":"track","payload":{"value":7}},"ref":null,"join_ref":null}"#))).await.unwrap();
    let push = read(&mut peer).await;
    ack(&mut peer, &push, "error").await;
    let reply = regression_next(&mut stream).await.unwrap();
    assert_eq!(reply.ref_field, push.ref_field);
    assert_eq!(reply.join_ref, joined.join_ref);
    let rp_supabase_realtime::message::ProtocolPayload::PhxReply(
        rp_supabase_realtime::message::phx_reply::PhxReply::Error(error),
    ) = reply.payload
    else {
        panic!("presence rejection must reach the consumer as a protocol reply")
    };
    assert_eq!(error.reason, "denied");
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Ready);
    stream.shutdown().await;
}

#[tokio::test]
async fn regression_postgres_system_error_allows_later_recovery() {
    let (_listener, mut stream, mut tx, mut peer) = setup(options()).await;
    let mut intended = join();
    let rp_supabase_realtime::message::ProtocolPayload::PhxJoin(config) = &mut intended.payload
    else {
        panic!("join")
    };
    config
        .config
        .postgres_changes
        .push(rp_supabase_realtime::message::phx_join::PostgresChanges {
            event: rp_supabase_realtime::message::phx_join::PostgresChangeEvent::Insert,
            schema: "public".to_owned(),
            table: "product_events".to_owned(),
            filter: None,
        });
    tx.send(Ok(intended)).await.unwrap();
    let joined = read(&mut peer).await;
    ack(&mut peer, &joined, "ok").await;
    regression_next(&mut stream).await.unwrap();
    for (status, message) in [
        ("error", "database temporarily unavailable"),
        ("ok", "subscribed"),
    ] {
        peer.send(Message::Text(format!(r#"{{"topic":"realtime:room","event":"system","payload":{{"status":"{status}","message":"{message}","extension":"postgres_changes","channel":"room"}},"ref":null,"join_ref":"{}"}}"#, joined.join_ref.as_ref().unwrap()).into())).await.unwrap();
        let event = regression_next(&mut stream).await;
        if status == "error" {
            match event {
                Err(rp_supabase_realtime::error::SupabaseRealtimeError::ChannelError {
                    reason,
                    ..
                }) => assert_eq!(reason, message),
                Ok(ProtocolMessage {
                    payload: rp_supabase_realtime::message::ProtocolPayload::System(system),
                    ..
                }) => assert_eq!(system.message, message),
                other => panic!("expected visible operational error, got {other:?}"),
            }
        } else {
            assert!(matches!(
                event.unwrap().payload,
                rp_supabase_realtime::message::ProtocolPayload::System(_)
            ));
        }
    }
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Ready);
    stream.shutdown().await;
}

async fn regression_join_rejection(reason: &str, retryable: bool) {
    let (listener, mut stream, mut tx, mut peer) = setup(ConnectionOptions {
        max_retries: 1,
        acknowledgement_timeout: Duration::from_millis(150),
        ..options()
    })
    .await;
    tx.send(Ok(join())).await.unwrap();
    let first = read(&mut peer).await;
    peer.send(Message::Text(format!(r#"{{"topic":"realtime:room","event":"phx_reply","payload":{{"status":"error","response":{{"reason":"{reason}"}}}},"ref":"{}","join_ref":"{}"}}"#, first.ref_field.as_ref().unwrap(), first.join_ref.as_ref().unwrap()).into())).await.unwrap();
    assert!(
        matches!(regression_next(&mut stream).await, Err(rp_supabase_realtime::error::SupabaseRealtimeError::ChannelError { reason: actual, .. }) if actual == reason),
        "the rejected join must expose the server reason"
    );
    if retryable {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("transient rejection must reconnect")
            .unwrap();
        let mut replacement = tokio_tungstenite::accept_async(socket).await.unwrap();
        let replay = tokio::time::timeout(Duration::from_secs(2), read(&mut replacement))
            .await
            .unwrap();
        assert_ne!(
            replay.join_ref, first.join_ref,
            "rejoin must use a fresh generation"
        );
        ack(&mut replacement, &replay, "ok").await;
        regression_next(&mut stream).await.unwrap();
        assert_eq!(
            *stream.status().borrow(),
            ConnectionStatus::Ready,
            "acknowledged rejoin restores readiness"
        );
    } else {
        assert!(
            tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .unwrap()
                .is_none(),
            "invalid join configuration must terminate the stream"
        );
        assert_eq!(
            *stream.status().borrow(),
            ConnectionStatus::Failed,
            "invalid join configuration is terminal"
        );
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .expect_err("invalid configuration must not reconnect");
    }
    stream.shutdown().await;
}

#[tokio::test]
async fn regression_realtime_restarting_join_retries() {
    regression_join_rejection("RealtimeRestarting: server restarting", true).await;
}

#[tokio::test]
async fn regression_initializing_project_join_retries() {
    regression_join_rejection("InitializingProjectConnection: database connecting", true).await;
}

#[tokio::test]
async fn regression_invalid_join_configuration_is_terminal() {
    regression_join_rejection("InvalidJoinPayload: invalid config", false).await;
}

fn regression_retryable_refresh_error() -> rp_supabase_realtime::error::SupabaseRealtimeError {
    let error = simd_json::from_slice::<rp_supabase_auth::types::ErrorSchema>(
        &mut br#"{"code":"unexpected_failure","message":"temporarily unavailable"}"#.to_vec(),
    )
    .unwrap();
    rp_supabase_realtime::error::SupabaseRealtimeError::RefreshStreamError(
        rp_supabase_auth::jwt_stream::RefreshStreamError::AuthError(
            rp_supabase_auth::error::AuthError::Api {
                status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
                error: Box::new(error),
            },
        ),
    )
}

#[tokio::test]
async fn regression_retryable_refresh_error_keeps_healthy_socket() {
    let (_listener, mut stream, mut tx, mut peer) = setup(options()).await;
    regression_join(&mut stream, &mut tx, &mut peer).await;
    tx.send(Err(regression_retryable_refresh_error()))
        .await
        .unwrap();
    assert!(matches!(
        regression_next(&mut stream).await,
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::RefreshStreamError(_))
    ));
    let token = decode(
        r#"{"topic":"realtime:room","event":"access_token","payload":{"access_token":"refreshed"},"ref":null,"join_ref":null}"#,
    );
    tx.send(Ok(token))
        .await
        .expect("retryable refresh must keep polling authentication");
    let token = tokio::time::timeout(Duration::from_secs(2), read(&mut peer))
        .await
        .unwrap();
    let rp_supabase_realtime::message::ProtocolPayload::AccessToken(token) = token.payload else {
        panic!("expected refreshed token on original socket")
    };
    assert_eq!(token.access_token, "refreshed");
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Ready);
    stream.shutdown().await;
}

#[tokio::test]
async fn regression_retryable_refresh_error_keeps_reconnect_backoff() {
    let (listener, mut stream, mut tx, mut peer) = setup(ConnectionOptions {
        initial_backoff: Duration::from_millis(200),
        max_backoff: Duration::from_millis(200),
        ..options()
    })
    .await;
    regression_join(&mut stream, &mut tx, &mut peer).await;
    let mut status = stream.status();
    drop(peer);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !matches!(*status.borrow(), ConnectionStatus::Reconnecting { .. }) {
            status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    tx.send(Err(regression_retryable_refresh_error()))
        .await
        .unwrap();
    loop {
        match regression_next(&mut stream).await {
            Err(rp_supabase_realtime::error::SupabaseRealtimeError::RefreshStreamError(_)) => break,
            Err(rp_supabase_realtime::error::SupabaseRealtimeError::Transport(_)) => {}
            other => panic!("expected the retryable refresh error, got {other:?}"),
        }
    }
    tx.send(Ok(decode(r#"{"topic":"realtime:room","event":"access_token","payload":{"access_token":"after-refresh-retry"},"ref":null,"join_ref":null}"#))).await.expect("backoff must survive retryable refresh");
    let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut peer = tokio_tungstenite::accept_async(socket).await.unwrap();
    let replay = read(&mut peer).await;
    let rp_supabase_realtime::message::ProtocolPayload::PhxJoin(config) = &replay.payload else {
        panic!("replayed join")
    };
    assert_eq!(config.access_token.as_deref(), Some("after-refresh-retry"));
    ack(&mut peer, &replay, "ok").await;
    regression_next(&mut stream).await.unwrap();
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Ready);
    stream.shutdown().await;
}

#[tokio::test]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "Tokio select uses modulo for fair branch scheduling"
)]
async fn regression_unfinished_websocket_handshakes_exhaust_retry_budget() {
    let (listener, mut stream, _tx, peer) = setup(ConnectionOptions {
        max_retries: 2,
        acknowledgement_timeout: Duration::from_millis(100),
        connect_timeout: Duration::from_millis(100),
        ..options()
    })
    .await;
    drop(peer);
    // Retain accepted TCP sockets without speaking HTTP: no port/refusal timing assumptions.
    let server = async {
        let mut sockets = Vec::new();
        for _ in 0_u8..2 {
            sockets.push(listener.accept().await.unwrap().0);
        }
        futures::future::pending::<()>().await;
        drop(sockets);
    };
    let client = async {
        loop {
            match stream.next().await {
                Some(Err(
                    rp_supabase_realtime::error::SupabaseRealtimeError::ReconnectExhausted,
                )) => break,
                Some(_) => {}
                None => panic!("handshake timeout must expose retry exhaustion"),
            }
        }
        assert_eq!(*stream.status().borrow(), ConnectionStatus::Failed);
    };
    tokio::pin!(server);
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            () = &mut server => panic!("blackhole server unexpectedly completed"),
            () = client => {}
        }
    })
    .await
    .expect("incomplete handshakes must consume the finite retry budget");
    stream.shutdown().await;
}

#[tokio::test]
async fn regression_backpressured_write_does_not_suspend_heartbeat_deadline() {
    let (_listener, mut stream, mut tx, mut peer) = setup(ConnectionOptions {
        max_retries: 0,
        heartbeat_interval: Duration::from_millis(20),
        acknowledgement_timeout: Duration::from_millis(100),
        write_timeout: Duration::from_millis(100),
        ..options()
    })
    .await;
    regression_join(&mut stream, &mut tx, &mut peer).await;
    let heartbeat = read(&mut peer).await;
    assert_eq!(heartbeat.topic, "phoenix");
    // Stop reading with a heartbeat already pending. A 32 MiB frame exceeds local
    // TCP buffering; serialization and socket writes must not disable that deadline.
    let payload = "x".repeat(32_usize * 1024 * 1024);
    tx.send(Ok(decode(&format!(r#"{{"topic":"realtime:room","event":"broadcast","payload":{{"type":"broadcast","event":"large","payload":{{"data":"{payload}"}}}},"ref":null,"join_ref":null}}"#)))).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match stream.next().await {
                Some(Err(
                    rp_supabase_realtime::error::SupabaseRealtimeError::ReconnectExhausted,
                )) => break,
                Some(Err(_)) => {}
                other => panic!("expected bounded transport failure, got {other:?}"),
            }
        }
    })
    .await
    .expect("backpressure must not suspend heartbeat/write deadlines");
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Failed);
    tokio::time::timeout(Duration::from_millis(200), stream.shutdown())
        .await
        .unwrap();
}

#[tokio::test]
async fn regression_heartbeat_blackhole_exhausts_budget_and_shutdown_completes() {
    let (_listener, mut stream, _tx, mut peer) = setup(ConnectionOptions {
        max_retries: 0,
        heartbeat_interval: Duration::from_millis(20),
        acknowledgement_timeout: Duration::from_millis(100),
        ..options()
    })
    .await;
    assert_eq!(read(&mut peer).await.topic, "phoenix");
    assert!(matches!(
        regression_next(&mut stream).await,
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::HeartbeatTimeout)
    ));
    assert!(matches!(
        regression_next(&mut stream).await,
        Err(rp_supabase_realtime::error::SupabaseRealtimeError::ReconnectExhausted)
    ));
    tokio::time::timeout(Duration::from_millis(200), stream.shutdown())
        .await
        .unwrap();
    assert_eq!(*stream.status().borrow(), ConnectionStatus::Closed);
}

#[tokio::test]
#[expect(
    clippy::integer_division_remainder_used,
    clippy::large_futures,
    reason = "Tokio select schedules both bounded loopback peers in one test task"
)]
async fn regression_initial_handshake_has_explicit_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "ws://{}/realtime/v1/websocket",
        listener.local_addr().unwrap()
    )
    .parse()
    .unwrap();
    let connection = RealtimeBaseConnection::new(url).with_options(ConnectionOptions {
        connect_timeout: Duration::from_millis(50),
        ..options()
    });
    let server = async {
        let (_socket, _) = listener.accept().await.unwrap();
        futures::future::pending::<()>().await;
    };
    let client = async {
        let result = connection.connect(futures::stream::pending()).await;
        assert!(matches!(
            result,
            Err(rp_supabase_realtime::error::SupabaseRealtimeError::ConnectTimeout)
        ));
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            () = server => panic!("blackhole server unexpectedly completed"),
            () = client => {}
        }
    })
    .await
    .unwrap();
}
