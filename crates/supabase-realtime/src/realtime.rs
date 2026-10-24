use crate::message;
use core::marker::PhantomData;
use futures::future::Either;
use serde::de::DeserializeOwned;

use futures::{SinkExt as _, Stream, StreamExt as _};
use rp_supabase_auth::types::LoginCredentials;

use crate::driver::{ConnectionOptions, RealtimeStream};
use crate::error::SupabaseRealtimeError;
use crate::message::access_token::AccessToken;
use crate::message::phx_join::PostgresChanges;
use crate::message::phx_reply::PhxReply;
use crate::message::postgres_changes::PostgresChange;
use crate::message::presence_inner::PresenceInner;
use crate::message::{ProtocolMessage, ProtocolPayload, broadcast, phx_join};

pub struct DbUpdates;
pub struct Broadcast;
pub struct Presence;

pub struct RealtimeConnectionClient<T> {
    tx: futures::channel::mpsc::Sender<ProtocolPayload>,
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
                private: false,
                broadcast: phx_join::BroadcastConfig {
                    self_item: false,
                    ack: false,
                },
                presence: phx_join::PresenceConfig {
                    enabled: false,
                    key: String::new(),
                },
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
                private: false,
                broadcast: phx_join::BroadcastConfig {
                    self_item: false,
                    ack: false,
                },
                presence: phx_join::PresenceConfig {
                    enabled: true,
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
                private: false,
                broadcast: join,
                presence: phx_join::PresenceConfig {
                    enabled: false,
                    key: String::new(),
                },
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
    options: ConnectionOptions,
    http: Option<reqwest::Client>,
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
    fn with_topic(config: rp_supabase_auth::jwt_stream::SupabaseAuthConfig, topic: &str) -> Self {
        Self {
            topic: ["realtime", topic].join(":"),
            config,
            _t: PhantomData,
            options: ConnectionOptions::default(),
            http: None,
        }
    }

    #[must_use]
    pub const fn with_options(mut self, options: ConnectionOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn with_auth_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// Connects an authenticated realtime channel and starts its outgoing streams.
    ///
    /// # Errors
    ///
    /// Returns an error if URL construction or the websocket connection fails.
    /// Authentication failures are delivered through the returned stream.
    #[tracing::instrument(skip_all, err)]
    pub async fn connect(
        mut self,
        login_info: LoginCredentials,
    ) -> Result<(RealtimeStream, RealtimeConnectionClient<T>), SupabaseRealtimeError> {
        let jwt = rp_supabase_auth::jwt_stream::JwtStream::new(self.config.clone());
        let auth_stream = match self.http.take() {
            Some(http) => jwt.sign_in_with_client(login_info, http)?,
            None => jwt.sign_in(login_info)?,
        };
        self.connect_with_auth_stream(auth_stream).await
    }

    /// Connect using an existing OAuth, anonymous, or MFA-upgraded session.
    ///
    /// # Errors
    /// Returns invalid-session, URL, or WebSocket setup errors.
    pub async fn connect_with_session(
        mut self,
        session: rp_supabase_auth::types::AccessTokenResponseSchema,
    ) -> Result<(RealtimeStream, RealtimeConnectionClient<T>), SupabaseRealtimeError> {
        let jwt = rp_supabase_auth::jwt_stream::JwtStream::new(self.config.clone());
        let auth_stream = match self.http.take() {
            Some(http) => jwt.with_session_with_client(session, http)?,
            None => jwt.with_session(session)?,
        };
        self.connect_with_auth_stream(auth_stream).await
    }

    /// Connect with a refresh stream configured by the caller, including its session store.
    ///
    /// # Errors
    /// Returns URL or WebSocket setup errors. Authentication errors arrive on the stream.
    pub async fn connect_with_auth_stream(
        self,
        auth_stream: rp_supabase_auth::jwt_stream::JwtRefreshStream,
    ) -> Result<(RealtimeStream, RealtimeConnectionClient<T>), SupabaseRealtimeError> {
        let mut realtime_url = self.config.url.join("realtime/v1/websocket")?;
        realtime_url
            .query_pairs_mut()
            .append_pair("apikey", &self.config.api_key)
            .append_pair("vsn", "1.0.0");
        let mut latest_access_token = String::new();

        let (tx, rx) = futures::channel::mpsc::channel(self.options.command_capacity.max(1));
        let topic = self.topic.clone();
        let input_stream = rx
            .map(move |item| {
                Ok(message::ProtocolMessage {
                    topic: topic.clone(),
                    payload: item,
                    ref_field: None,
                    join_ref: None,
                })
            })
            .boxed();

        let topic = self.topic.clone();
        let mut access_token_stream = {
            auth_stream
                .map(move |item| {
                    let token = item.map_err(SupabaseRealtimeError::from)?;
                    let access_token = token
                        .access_token
                        .ok_or(SupabaseRealtimeError::JwtStreamClosedUnexpectedly)?;
                    Ok(message::ProtocolMessage {
                        topic: topic.clone(),
                        payload: message::ProtocolPayload::AccessToken(AccessToken {
                            access_token,
                        }),
                        ref_field: None,
                        join_ref: None,
                    })
                })
                .chain(futures::stream::once(async {
                    Err(SupabaseRealtimeError::JwtStreamClosedUnexpectedly)
                }))
                .boxed()
        };
        // Do not send queued joins until authentication supplies a usable token.
        // Authentication errors remain observable while its own retry policy runs.
        let input_stream = async_stream::stream! {
            while let Some(item) = access_token_stream.next().await {
                let authenticated = item.is_ok();
                let terminal = matches!(&item, Err(SupabaseRealtimeError::JwtStreamClosedUnexpectedly));
                yield item;
                if terminal { return; }
                if authenticated { break; }
            }
            let mut inputs = futures::stream::select_all([input_stream, access_token_stream]);
            while let Some(item) = inputs.next().await {
                yield item;
            }
        }.boxed().map(
            move |mut item| {
                if let Ok(item) = &mut item {
                    if let message::ProtocolPayload::AccessToken(at) = &mut item.payload {
                        latest_access_token = at.access_token.clone();
                    }
                    item.set_access_token(&latest_access_token);
                }
                item
            },
        );

        let client = RealtimeConnectionClient {
            tx,
            _t: PhantomData,
        };
        let output_stream = RealtimeBaseConnection::new(realtime_url)
            .with_options(self.options)
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
    options: ConnectionOptions,
}

impl RealtimeBaseConnection {
    #[must_use]
    pub fn new(url: url::Url) -> Self {
        Self {
            url,
            options: ConnectionOptions::default(),
        }
    }

    #[must_use]
    pub const fn with_options(mut self, options: ConnectionOptions) -> Self {
        self.options = options;
        self
    }

    /// Starts the reconnecting protocol driver.
    ///
    /// # Errors
    /// Returns an error if the initial websocket handshake fails.
    pub async fn connect<S>(self, input: S) -> Result<RealtimeStream, SupabaseRealtimeError>
    where
        S: Stream<Item = RealtimeStreamType> + Unpin + Send + 'static,
    {
        crate::driver::connect(self.url, input, self.options).await
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
