use alloc::sync::Arc;
use core::marker::PhantomData;
use core::task::Poll;
use futures::future::Either;
use serde::de::DeserializeOwned;

use fastwebsockets::{Frame, WebSocketError};
use futures::stream::FuturesUnordered;
use futures::{SinkExt as _, Stream, StreamExt as _};
use rp_supabase_auth::types::LoginCredentials;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_stream::wrappers::IntervalStream;

use crate::connection::WsSupabaseConnection;
use crate::error::SupabaseRealtimeError;
use crate::message::access_token::AccessToken;
use crate::message::phx_join::PostgresChanges;
use crate::message::phx_reply::PhxReply;
use crate::message::postgres_changes::PostgresChange;
use crate::message::presence_inner::PresenceInner;
use crate::message::{ProtocolMessage, ProtocolPayload, broadcast, phx_join};
use crate::{connection, error, message};

pub struct DbUpdates;
pub struct Broadcast;
pub struct Presence;

pub struct RealtimeConnectionClient<T> {
    tx: futures::channel::mpsc::UnboundedSender<ProtocolPayload>,
    _t: PhantomData<T>,
}

impl RealtimeConnectionClient<DbUpdates> {
    /// Subscribe to postgres changes
    ///
    /// # Errors
    /// - if message cannot be delivered
    pub async fn subscribe_to_changes(
        &mut self,
        join: Vec<PostgresChanges>,
    ) -> Result<(), futures::channel::mpsc::SendError> {
        let join = phx_join::PhxJoin {
            config: phx_join::JoinConfig {
                broadcast: phx_join::BroadcastConfig {
                    self_item: false,
                    ack: false,
                },
                presence: phx_join::PresenceConfig { key: String::new() },
                postgres_changes: join,
            },
            access_token: None,
        };
        self.tx.send(ProtocolPayload::PhxJoin(join)).await
    }
}

impl RealtimeConnectionClient<Presence> {
    /// Join a presence channel
    ///
    /// # Errors
    /// - if message cannot be delivered
    pub async fn join(
        &mut self,
        unique_user_key: Option<String>,
    ) -> Result<(), futures::channel::mpsc::SendError> {
        let join = phx_join::PhxJoin {
            config: phx_join::JoinConfig {
                broadcast: phx_join::BroadcastConfig {
                    self_item: false,
                    ack: false,
                },
                presence: phx_join::PresenceConfig {
                    key: unique_user_key.unwrap_or_default(),
                },
                postgres_changes: Vec::new(),
            },
            access_token: None,
        };
        self.tx.send(ProtocolPayload::PhxJoin(join)).await
    }

    /// Start tracking new state
    ///
    /// # Errors
    /// - if message cannot be deserialized into json
    /// - if message cannot be delivered
    pub async fn track<T: serde::Serialize>(&mut self, item: &T) -> Result<(), PresenceError> {
        let mut item = simd_json::to_vec(&item)?;
        let payload = simd_json::to_owned_value(&mut item)?;
        let item = ProtocolPayload::PresenceInner(PresenceInner {
            r#type: "presence".to_owned(),
            payload: message::presence_inner::PresenceInnerPayload::Track(payload),
        });
        self.tx.send(item).await?;
        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum PresenceError {
    #[error("json serialization error")]
    SimdError(#[from] simd_json::Error),
    #[error("mpsc send error")]
    MpscError(#[from] futures::channel::mpsc::SendError),
}

impl RealtimeConnectionClient<Broadcast> {
    /// Join a broadcast channel
    ///
    /// # Errors
    /// - if message cannot be delivered
    pub async fn join(
        &mut self,
        join: phx_join::BroadcastConfig,
    ) -> Result<(), futures::channel::mpsc::SendError> {
        let join = phx_join::PhxJoin {
            config: phx_join::JoinConfig {
                broadcast: join,
                presence: phx_join::PresenceConfig { key: String::new() },
                postgres_changes: Vec::new(),
            },
            access_token: None,
        };
        self.tx.send(ProtocolPayload::PhxJoin(join)).await
    }

    /// Broadcast a new message over the channel
    ///
    /// # Errors
    /// - if message cannot be delivered
    pub async fn broadcast(
        &mut self,
        msg: broadcast::Broadcast,
    ) -> Result<(), futures::channel::mpsc::SendError> {
        self.tx.send(ProtocolPayload::Broadcast(msg)).await
    }
}

pub struct RealtimeConnection<T> {
    topic: String,
    config: rp_supabase_auth::jwt_stream::SupabaseAuthConfig,
    _t: PhantomData<T>,
}

type RealtimeStreamType = Result<ProtocolMessage, SupabaseRealtimeError>;

impl RealtimeConnection<DbUpdates> {
    /// Creates a channel that receives postgres changes.
    #[must_use]
    pub fn db_changes(config: rp_supabase_auth::jwt_stream::SupabaseAuthConfig) -> Self {
        Self::with_topic(config, "table-db-changes")
    }
}

impl RealtimeConnection<Broadcast> {
    /// Creates a broadcast channel on `topic`.
    #[must_use]
    pub fn broadcast(
        config: rp_supabase_auth::jwt_stream::SupabaseAuthConfig,
        topic: &str,
    ) -> Self {
        Self::with_topic(config, topic)
    }
}

impl<T> RealtimeConnection<T> {
    const HEARTBEAT_PERIOD: core::time::Duration = core::time::Duration::from_secs(20);

    fn with_topic(config: rp_supabase_auth::jwt_stream::SupabaseAuthConfig, topic: &str) -> Self {
        Self {
            topic: ["realtime", topic].join(":"),
            config,
            _t: PhantomData,
        }
    }

    fn heartbeat_stream() -> futures::stream::BoxStream<'static, RealtimeStreamType> {
        let mut interval = tokio::time::interval(Self::HEARTBEAT_PERIOD);
        interval.reset();
        IntervalStream::new(interval)
            .fuse()
            .map(move |_s| message::ProtocolMessage {
                topic: "phoenix".to_owned(),
                payload: message::ProtocolPayload::Heartbeat(message::heartbeat::Heartbeat),
                ref_field: None,
                join_ref: None,
            })
            .map(Ok)
            .boxed()
    }

    /// Connects an authenticated realtime channel and starts its outgoing streams.
    ///
    /// # Errors
    ///
    /// Returns an error if URL construction, authentication, or the websocket connection fails.
    #[tracing::instrument(skip_all, err)]
    pub async fn connect(
        self,
        login_info: LoginCredentials,
    ) -> Result<
        (
            impl Stream<Item = RealtimeStreamType>,
            RealtimeConnectionClient<T>,
        ),
        SupabaseRealtimeError,
    > {
        let anon_key = &self.config.api_key;
        let realtime_url = self
            .config
            .url
            .join(format!("realtime/v1/websocket?apikey={anon_key}&vsn=1.0.0").as_str())?;

        let mut auth_stream = rp_supabase_auth::jwt_stream::JwtStream::new(self.config.clone())
            .sign_in(login_info)?;
        let mut latest_access_token = loop {
            match auth_stream.next().await {
                Some(Ok(new_latest_access_token)) => {
                    let Some(access_token) = new_latest_access_token.access_token else {
                        tracing::error!("access token was not present!");
                        continue;
                    };
                    break access_token;
                }
                Some(Err(err)) => {
                    tracing::error!(?err, "initial jwt fetch err");
                }
                None => return Err(error::SupabaseRealtimeError::JwtStreamClosedUnexpectedly),
            }
        };

        let mut ref_counter = 0_u64;
        let mut join_ref_counter = 0_u64;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let topic = self.topic.clone();
        let input_stream = rx
            .map(move |item| {
                ref_counter = ref_counter.wrapping_add(1);
                join_ref_counter = join_ref_counter.wrapping_add(1);
                message::ProtocolMessage {
                    topic: topic.clone(),
                    payload: item,
                    ref_field: Some(ref_counter.to_string()),
                    join_ref: Some(join_ref_counter.to_string()),
                }
            })
            .map(Ok)
            .boxed();

        let heartbeat_stream = Self::heartbeat_stream();

        let topic = self.topic.clone();
        let access_token_stream = {
            auth_stream
                .filter_map(move |item| {
                    let topic = topic.clone();
                    async move {
                        item.map(|item| {
                            if let Some(access_token) = item.access_token {
                                return Some(message::ProtocolMessage {
                                    topic: topic.clone(),
                                    payload: message::ProtocolPayload::AccessToken(AccessToken {
                                        access_token,
                                    }),
                                    ref_field: None,
                                    join_ref: None,
                                });
                            }
                            None
                        })
                        .map_err(SupabaseRealtimeError::from)
                        .transpose()
                    }
                })
                .boxed()
        };
        let input_stream =
            futures::stream::select_all([input_stream, heartbeat_stream, access_token_stream])
                .map(move |mut item| {
                    if let Ok(item) = &mut item {
                        if let message::ProtocolPayload::AccessToken(at) = &mut item.payload {
                            latest_access_token = at.access_token.clone();
                        }
                        item.set_access_token(&latest_access_token);
                    }
                    item
                })
                .map(move |mut item| {
                    ref_counter = ref_counter.saturating_add(1);
                    if let Ok(item) = &mut item {
                        item.ref_field = Some(ref_counter.to_string());
                    }
                    item
                });

        let client = RealtimeConnectionClient {
            tx,
            _t: PhantomData,
        };
        let output_stream = RealtimeBaseConnection::new(realtime_url)
            .connect(input_stream)
            .await?;
        Ok((output_stream, client))
    }
}

/// Turns the output stream of a [`DbUpdates`] connection into typed postgres changes.
///
/// The returned stream drops successful protocol messages such as heartbeats,
/// `ok` replies, and `ok` system messages. It yields
/// [`SupabaseRealtimeError::ChannelError`] when the server rejects the join or
/// the subscription, or reports a channel error. It also yields an error when
/// the connection fails or when a row does not decode into `T`.
#[expect(
    clippy::impl_trait_in_params,
    reason = "Callers name only the row type: `typed_changes::<Row>(stream)`"
)]
pub fn typed_changes<T: DeserializeOwned>(
    stream: impl Stream<Item = RealtimeStreamType>,
) -> impl Stream<Item = Result<PostgresChange<T>, SupabaseRealtimeError>> {
    stream.filter_map(|msg| futures::future::ready(typed_change(msg)))
}

fn typed_change<T: DeserializeOwned>(
    msg: RealtimeStreamType,
) -> Option<Result<PostgresChange<T>, SupabaseRealtimeError>> {
    let ProtocolMessage { topic, payload, .. } = match msg {
        Ok(msg) => msg,
        Err(err) => return Some(Err(err)),
    };
    let reason = match payload {
        ProtocolPayload::PostgresChanges(payload) => return Some(payload.data.into_change::<T>()),
        ProtocolPayload::PhxReply(PhxReply::Error(reply)) => reply.reason,
        ProtocolPayload::System(system) if system.status != "ok" => system.message,
        ProtocolPayload::PhxError(_) => "the server reported a channel error".to_owned(),
        ProtocolPayload::Heartbeat(_)
        | ProtocolPayload::AccessToken(_)
        | ProtocolPayload::PhxJoin(_)
        | ProtocolPayload::PhxClose(_)
        | ProtocolPayload::PhxReply(PhxReply::Ok(_))
        | ProtocolPayload::Broadcast(_)
        | ProtocolPayload::PresenceInner(_)
        | ProtocolPayload::PresenceState(_)
        | ProtocolPayload::PresenceDiff(_)
        | ProtocolPayload::System(_) => return None,
    };
    Some(Err(SupabaseRealtimeError::ChannelError { topic, reason }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceParsed<T> {
    pub metas: Vec<PresenceMetaParsed<T>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceMetaParsed<T> {
    pub phx_ref: String,
    /// Every key the client tracked, decoded as `T`.
    pub payload: T,
}

impl RealtimeConnection<Presence> {
    /// Creates a presence channel on `topic`.
    #[must_use]
    pub fn presence(config: rp_supabase_auth::jwt_stream::SupabaseAuthConfig, topic: &str) -> Self {
        Self::with_topic(config, topic)
    }

    /// Connects a presence channel and tracks its current presence state.
    ///
    /// # Errors
    ///
    /// Returns an error if authentication or the realtime connection fails.
    #[tracing::instrument(skip_all, err)]
    pub async fn connect_with_state_tracking<T: DeserializeOwned>(
        self,
        login_info: LoginCredentials,
    ) -> Result<
        (
            impl Stream<
                Item = Result<Either<PresenceParsed<T>, ProtocolMessage>, SupabaseRealtimeError>,
            >,
            RealtimeConnectionClient<Presence>,
        ),
        SupabaseRealtimeError,
    > {
        let (stream, realtime_client) = self.connect(login_info).await?;

        let mut current_state = std::collections::HashMap::new();

        let stream = stream.map(move |msg| {
            match msg {
                Ok(ProtocolMessage {
                    payload: ProtocolPayload::PresenceState(state),
                    ..
                }) => {
                    // Reset state with new presence state
                    current_state = state.0;
                    let parsed_state = current_state
                        .iter()
                        .map(|(key, presence)| {
                            let metas = presence
                                .metas
                                .iter()
                                .map(|meta| {
                                    let mut payload_bytes = simd_json::to_vec(&meta.payload)?;
                                    let payload = simd_json::from_slice(&mut payload_bytes)?;
                                    Ok::<_, simd_json::Error>(PresenceMetaParsed {
                                        phx_ref: meta.phx_ref.clone(),

                                        payload,
                                    })
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            Ok::<_, simd_json::Error>((key.clone(), metas))
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    Ok(Either::Left(PresenceParsed {
                        metas: parsed_state
                            .into_iter()
                            .flat_map(|(_, metas)| metas)
                            .collect(),
                    }))
                }
                Ok(ProtocolMessage {
                    payload: ProtocolPayload::PresenceDiff(diff),
                    ..
                }) => {
                    // Handle joins
                    for (key, presence) in diff.joins {
                        current_state.insert(key, presence);
                    }

                    // Handle leaves
                    for (key, _) in diff.leaves {
                        current_state.remove(&key);
                    }

                    // Convert current state to parsed format
                    let parsed_state = current_state
                        .iter()
                        .map(|(key, presence)| {
                            let metas = presence
                                .metas
                                .iter()
                                .map(|meta| {
                                    let mut payload_bytes = simd_json::to_vec(&meta.payload)?;
                                    let payload = simd_json::from_slice(&mut payload_bytes)?;
                                    Ok::<_, simd_json::Error>(PresenceMetaParsed {
                                        phx_ref: meta.phx_ref.clone(),

                                        payload,
                                    })
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            Ok::<_, simd_json::Error>((key.clone(), metas))
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    Ok(Either::Left(PresenceParsed {
                        metas: parsed_state
                            .into_iter()
                            .flat_map(|(_, metas)| metas)
                            .collect(),
                    }))
                }
                Ok(msg) => Ok(Either::Right(msg)),
                Err(err) => Err(err),
            }
        });

        Ok((stream, realtime_client))
    }
}
pub struct RealtimeBaseConnection {
    url: url::Url,
}

impl RealtimeBaseConnection {
    #[must_use]
    pub const fn new(url: url::Url) -> Self {
        Self { url }
    }

    /// Connect to a supabase realtime channel
    ///
    /// # Errors
    /// - cannot connect
    pub async fn connect<S: Stream<Item = RealtimeStreamType> + Unpin>(
        self,
        mut input_stream: S,
    ) -> Result<impl Stream<Item = RealtimeStreamType>, error::SupabaseRealtimeError> {
        tracing::info!(url =? self.url.as_str(), "Starting RealtimeConnection::connect");

        let con = Arc::new(Mutex::new(connection::connect(&self.url).await?));
        tracing::info!("WebSocket connection established");

        let mut write_futures = FuturesUnordered::new();
        let mut read_futures = FuturesUnordered::new();
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let read_task = {
            let con = Arc::clone(&con);
            async move {
                let con = Arc::clone(&con);
                read_from_ws(&con, tx).await
            }
        };
        read_futures.push(read_task);

        let stream_to_return = futures::stream::poll_fn(move |cx| {
            match input_stream.poll_next_unpin(cx) {
                Poll::Ready(Some(message_to_send)) => {
                    let con = Arc::clone(&con);
                    match message_to_send {
                        Ok(message) => {
                            write_futures.push(async move {
                                let con = Arc::clone(&con);
                                send(message, &con).await
                            });
                        }
                        Err(err) => {
                            cx.waker().wake_by_ref();
                            return Poll::Ready(Some(Err(err)));
                        }
                    }
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => {}
            }

            match read_futures.poll_next_unpin(cx) {
                Poll::Ready(result) => {
                    tracing::error!(?result, "Read task completed");
                    return Poll::Ready(None);
                }
                Poll::Pending => {}
            }

            match write_futures.poll_next_unpin(cx) {
                Poll::Ready(Some(res)) => match res {
                    Ok(()) => {
                        tracing::debug!("Message sent successfully");
                    }
                    Err(err) => {
                        tracing::warn!(?err, "Error sending message");
                        if let SupabaseRealtimeError::WebsocketError(err) = &err
                            && let Err(err) = is_irrecoverable_ws_err(err)
                        {
                            tracing::error!(?err, "Irrecoverable error");
                            return Poll::Ready(None);
                        }
                        cx.waker().wake_by_ref();
                        return Poll::Ready(Some(Err(err)));
                    }
                },
                Poll::Ready(None) | Poll::Pending => {}
            }

            match rx.poll_next_unpin(cx) {
                Poll::Ready(Some(item)) => {
                    tracing::debug!(?item, "Received item");
                    cx.waker().wake_by_ref();
                    Poll::Ready(Some(Ok(item)))
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        });
        Ok(stream_to_return)
    }
}

async fn read_from_ws(
    con: &Mutex<WsSupabaseConnection>,
    mut tx: futures::channel::mpsc::UnboundedSender<ProtocolMessage>,
) -> Result<(), WebSocketError> {
    tracing::info!("Starting read_from_ws task");
    let duration = core::time::Duration::from_millis(100);
    loop {
        let mut con = con.lock().await;
        let Ok(frame) = timeout(duration, con.read_frame()).await else {
            continue;
        };
        drop(con);

        let mut frame = match frame {
            Ok(frame) => frame,
            Err(err) => {
                if is_irrecoverable_ws_err(&err).is_err() {
                    return Err(err);
                }
                continue;
            }
        };
        let repr = String::from_utf8_lossy(&frame.payload);
        tracing::debug!(?repr, "Received frame");

        let from_slice = simd_json::from_slice(frame.payload.to_mut());
        match from_slice {
            Ok(item) => {
                let _res = tx.send(item).await;
            }
            Err(err) => {
                let repr = String::from_utf8_lossy(&frame.payload);
                tracing::error!(?err, payload = ?repr, "Error deserializing data");
            }
        }
    }
}

async fn send(
    message_to_send: ProtocolMessage,
    con: &Mutex<WsSupabaseConnection>,
) -> Result<(), error::SupabaseRealtimeError> {
    tracing::debug!(?message_to_send, "Sending message");
    let message_bytes = simd_json::to_vec(&message_to_send)?;
    let payload = fastwebsockets::Payload::<'static>::Owned(message_bytes);
    let frame = Frame::<'static>::text(payload);
    let mut con = con.lock().await;
    con.write_frame(frame).await?;
    drop(con);
    Ok(())
}

#[tracing::instrument(skip_all, err)]
fn is_irrecoverable_ws_err(err: &WebSocketError) -> Result<(), &WebSocketError> {
    match err {
        // Here we check for a transient IO error that might be recoverable.
        // For example, WouldBlock or TimedOut could allow for a retry.
        WebSocketError::IoError(io_err)
            if io_err.kind() == std::io::ErrorKind::WouldBlock
                || io_err.kind() == std::io::ErrorKind::TimedOut =>
        {
            // Log and attempt recovery (retry, backoff, etc.)
            tracing::warn!(
                "Transient IO error encountered: {}. Attempting recovery...",
                io_err
            );
            Ok(())
        }

        // All the protocol-level or fatal errors are treated as irrecoverable.
        WebSocketError::UnexpectedEOF
        | WebSocketError::InvalidFragment
        | WebSocketError::InvalidUTF8
        | WebSocketError::InvalidContinuationFrame
        | WebSocketError::InvalidStatusCode(_)
        | WebSocketError::InvalidUpgradeHeader
        | WebSocketError::InvalidConnectionHeader
        | WebSocketError::ConnectionClosed
        | WebSocketError::InvalidCloseFrame
        | WebSocketError::InvalidCloseCode
        | WebSocketError::ReservedBitsNotZero
        | WebSocketError::ControlFrameFragmented
        | WebSocketError::PingFrameTooLarge
        | WebSocketError::FrameTooLarge
        | WebSocketError::InvalidSecWebsocketVersion
        | WebSocketError::InvalidValue
        | WebSocketError::MissingSecWebSocketKey
        // Optionally, if features are enabled:
        | WebSocketError::HTTPError(_)
        | WebSocketError::SendError(_)
        // For any other errors, decide as needed.
        | WebSocketError::IoError(_)
        => {
            // Propagate irrecoverable errors immediately.
            Err(err)
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "Allowed in test code for simplicity")]
mod tests {
    use super::*;

    fn decode(
        json: &str,
    ) -> Option<Result<PostgresChange<simd_json::OwnedValue>, SupabaseRealtimeError>> {
        let mut bytes = json.as_bytes().to_vec();
        let msg: ProtocolMessage = simd_json::from_slice(&mut bytes).unwrap();
        typed_change(Ok(msg))
    }

    fn channel_error(json: &str) -> (String, String) {
        match decode(json) {
            Some(Err(SupabaseRealtimeError::ChannelError { topic, reason })) => {
                Some((topic, reason))
            }
            _ => None,
        }
        .unwrap()
    }

    #[test]
    fn error_reply_yields_channel_error() {
        let (topic, reason) = channel_error(
            r#"{"event":"phx_reply","payload":{"response":{"reason":"Invalid JWT Token"},"status":"error"},"ref":"1","topic":"realtime:db"}"#,
        );
        assert_eq!(topic, "realtime:db");
        assert_eq!(reason, "Invalid JWT Token");
    }

    #[test]
    fn system_error_yields_channel_error() {
        let (_, reason) = channel_error(
            r#"{"event":"system","payload":{"channel":"db","extension":"postgres_changes","message":"bad filter","status":"error"},"ref":null,"topic":"realtime:db"}"#,
        );
        assert_eq!(reason, "bad filter");
    }

    #[test]
    fn system_timeout_yields_channel_error() {
        let (topic, reason) = channel_error(
            r#"{"event":"system","payload":{"channel":"db","extension":"postgres_changes","message":"Subscription timed out","status":"timeout"},"ref":null,"topic":"realtime:db"}"#,
        );
        assert_eq!(topic, "realtime:db");
        assert_eq!(reason, "Subscription timed out");
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct Row {
        id: i64,
        body: String,
    }

    fn message(json: &str) -> ProtocolMessage {
        let mut bytes = json.as_bytes().to_vec();
        simd_json::from_slice(&mut bytes).unwrap()
    }

    fn change(event: &str, record: &str, old_record: &str) -> ProtocolMessage {
        message(&format!(
            r#"{{"event":"postgres_changes","topic":"realtime:db","ref":null,
            "payload":{{"ids":[1],"data":{{"columns":[],
            "commit_timestamp":"2024-08-25T17:00:19.009Z","errors":null,
            "schema":"public","table":"messages","type":"{event}",
            "record":{record},"old_record":{old_record}}}}}}}"#
        ))
    }

    #[test]
    #[expect(clippy::panic, reason = "test asserts the exact output sequence")]
    fn typed_changes_yields_every_item_in_order() {
        let input = futures::stream::iter([
            Ok(message(
                r#"{"event":"phx_reply","payload":{"status":"ok","response":{}},"ref":"1","topic":"realtime:db"}"#,
            )),
            Ok(change("INSERT", r#"{"id":1,"body":"hi"}"#, "null")),
            Err(SupabaseRealtimeError::MpscSendError),
            Ok(message(
                r#"{"event":"phx_reply","payload":{"response":{"reason":"Invalid JWT Token"},"status":"error"},"ref":"2","topic":"realtime:db"}"#,
            )),
            Ok(change("INSERT", r#"{"id":"wrong","body":"hi"}"#, "null")),
            Ok(change(
                "UPDATE",
                r#"{"id":1,"body":"edited"}"#,
                r#"{"id":1}"#,
            )),
        ]);
        let output: Vec<_> = futures::executor::block_on(typed_changes::<Row>(input).collect());

        let [first, second, third, fourth, fifth] = <[_; 5]>::try_from(output).unwrap();
        match first {
            Ok(PostgresChange::Insert { record, .. }) => assert_eq!(
                record,
                Row {
                    id: 1,
                    body: "hi".to_owned()
                }
            ),
            other => panic!("expected insert: {other:?}"),
        }
        assert!(
            matches!(second, Err(SupabaseRealtimeError::MpscSendError)),
            "{second:?}"
        );
        match third {
            Err(SupabaseRealtimeError::ChannelError { topic, reason }) => {
                assert_eq!(topic, "realtime:db");
                assert_eq!(reason, "Invalid JWT Token");
            }
            other => panic!("expected channel error: {other:?}"),
        }
        assert!(
            matches!(fourth, Err(SupabaseRealtimeError::SerdeJsonError(_))),
            "{fourth:?}"
        );
        match fifth {
            Ok(PostgresChange::Update {
                record, old_record, ..
            }) => {
                assert_eq!(
                    record,
                    Row {
                        id: 1,
                        body: "edited".to_owned()
                    }
                );
                assert_eq!(old_record, simd_json::json!({"id": 1_i64}));
            }
            other => panic!("expected update: {other:?}"),
        }
    }

    #[test]
    fn phx_error_yields_channel_error() {
        let (topic, _) =
            channel_error(r#"{"event":"phx_error","payload":{},"ref":"1","topic":"realtime:db"}"#);
        assert_eq!(topic, "realtime:db");
    }

    #[test]
    fn successful_protocol_messages_are_skipped() {
        for json in [
            r#"{"event":"phx_reply","payload":{"status":"ok","response":{}},"ref":"1","topic":"realtime:db"}"#,
            r#"{"event":"system","payload":{"channel":"db","extension":"postgres_changes","message":"Subscribed to PostgreSQL","status":"ok"},"ref":null,"topic":"realtime:db"}"#,
            r#"{"event":"heartbeat","payload":{},"ref":"2","topic":"phoenix"}"#,
        ] {
            assert!(decode(json).is_none(), "{json}");
        }
    }
}
