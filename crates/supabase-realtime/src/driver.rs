use alloc::collections::BTreeMap;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;

use futures::{SinkExt as _, Stream, StreamExt as _};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use crate::connection::{self, WsSupabaseConnection};
use crate::error::SupabaseRealtimeError;
use crate::message::{ProtocolMessage, ProtocolPayload, phx_reply::PhxReply};

type Item = Result<ProtocolMessage, SupabaseRealtimeError>;

/// Bounded reconnect and liveness settings. Zero durations are clamped to one millisecond.
#[derive(Clone, Debug)]
pub struct ConnectionOptions {
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub heartbeat_interval: Duration,
    pub acknowledgement_timeout: Duration,
    /// Deadline for the complete TCP/TLS/websocket handshake.
    pub connect_timeout: Duration,
    /// Deadline for each websocket write, including replay and heartbeat writes.
    pub write_timeout: Duration,
    pub command_capacity: usize,
    pub event_capacity: usize,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            max_retries: 8,
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(20),
            acknowledgement_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
            command_capacity: 64,
            event_capacity: 128,
        }
    }
}

/// Current connection lifecycle. Ready means all desired joins have been acknowledged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connecting,
    Joining,
    Ready,
    Reconnecting { attempt: u32, delay: Duration },
    Failed,
    Closed,
}

/// Bounded event stream. Dropping it aborts the driver, authentication and backoff.
pub struct RealtimeStream {
    receiver: mpsc::Receiver<Item>,
    task: tokio::task::JoinHandle<()>,
    status: watch::Receiver<ConnectionStatus>,
    status_tx: watch::Sender<ConnectionStatus>,
}

impl RealtimeStream {
    /// Subscribe to the latest lifecycle state; intermediate transitions may coalesce.
    #[must_use]
    pub fn status(&self) -> watch::Receiver<ConnectionStatus> {
        self.status.clone()
    }

    /// Observe lifecycle state as a stream, starting with the current state.
    /// Like a watch receiver, slow consumers may coalesce intermediate transitions.
    pub fn statuses(&self) -> futures::stream::BoxStream<'static, ConnectionStatus> {
        futures::stream::unfold(
            (self.status.clone(), true),
            |(mut receiver, initial)| async move {
                if !initial && receiver.changed().await.is_err() {
                    return None;
                }
                let status = receiver.borrow_and_update().clone();
                Some((status, (receiver, false)))
            },
        )
        .boxed()
    }

    /// Cancels all work immediately, including an in-progress reconnect delay.
    pub async fn shutdown(&mut self) {
        self.task.abort();
        let _result = (&mut self.task).await;
        self.receiver.close();
        self.status_tx.send_replace(ConnectionStatus::Closed);
    }
}

impl Drop for RealtimeStream {
    fn drop(&mut self) {
        self.status_tx.send_replace(ConnectionStatus::Closed);
        self.task.abort();
    }
}

impl Stream for RealtimeStream {
    type Item = Item;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Item>> {
        self.receiver.poll_recv(cx)
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "Join acknowledgement, system readiness/retry and presence snapshot are independent protocol conditions"
)]
struct Join {
    message: ProtocolMessage,
    reference: String,
    acknowledged: bool,
    system_ready: bool,
    system_retrying: bool,
    presence: Option<ProtocolMessage>,
    snapshot: bool,
    deadline: Instant,
}

struct Registry {
    joins: BTreeMap<String, Join>,
    token: Option<String>,
    reference: u64,
}

impl Registry {
    fn next_ref(&mut self) -> String {
        self.reference = self.reference.saturating_add(1);
        self.reference.to_string()
    }

    fn prepare_join(&mut self, topic: &str, timeout: Duration) -> Option<ProtocolMessage> {
        let reference = self.next_ref();
        let join = self.joins.get_mut(topic)?;
        join.reference.clone_from(&reference);
        join.message.ref_field = Some(reference.clone());
        join.message.join_ref = Some(reference);
        if let Some(token) = &self.token {
            join.message.set_access_token(token);
        }
        join.acknowledged = false;
        join.snapshot = false;
        join.system_retrying = false;
        join.system_ready = matches!(&join.message.payload, ProtocolPayload::PhxJoin(j) if j.config.postgres_changes.is_empty());
        join.deadline = acknowledgement_deadline(timeout);
        Some(join.message.clone())
    }

    fn remember(&mut self, message: &ProtocolMessage, timeout: Duration) {
        match &message.payload {
            ProtocolPayload::PhxJoin(_) => {
                let presence = self
                    .joins
                    .remove(&message.topic)
                    .and_then(|join| join.presence);
                self.joins.insert(
                    message.topic.clone(),
                    Join {
                        message: message.clone(),
                        reference: String::new(),
                        acknowledged: false,
                        system_ready: false,
                        system_retrying: false,
                        presence,
                        snapshot: false,
                        deadline: acknowledgement_deadline(timeout),
                    },
                );
            }
            ProtocolPayload::AccessToken(token) => self.token = Some(token.access_token.clone()),
            ProtocolPayload::PresenceInner(_) => {
                if let Some(join) = self.joins.get_mut(&message.topic) {
                    join.presence = Some(message.clone());
                }
            }
            ProtocolPayload::PhxClose(_) => {
                self.joins.remove(&message.topic);
            }
            ProtocolPayload::Heartbeat(_)
            | ProtocolPayload::PhxReply(_)
            | ProtocolPayload::Broadcast(_)
            | ProtocolPayload::PresenceState(_)
            | ProtocolPayload::PresenceDiff(_)
            | ProtocolPayload::System(_)
            | ProtocolPayload::PhxError(_)
            | ProtocolPayload::PostgresChanges(_) => {}
        }
    }

    fn ready(&self) -> bool {
        !self.joins.is_empty()
            && self
                .joins
                .values()
                .all(|join| join.acknowledged && join.system_ready)
    }
}

async fn send(
    socket: &mut WsSupabaseConnection,
    message: &ProtocolMessage,
    timeout: Duration,
) -> Result<(), SupabaseRealtimeError> {
    let bytes = simd_json::to_vec(message)?;
    tokio::time::timeout(
        timeout,
        socket.send(Message::Text(
            String::from_utf8(bytes)
                .map_err(|_error| SupabaseRealtimeError::WsProcessingError)?
                .into(),
        )),
    )
    .await
    .map_err(|_error| SupabaseRealtimeError::WriteTimeout)??;
    Ok(())
}

fn unavailable(topic: String) -> SupabaseRealtimeError {
    SupabaseRealtimeError::ChannelError {
        topic,
        reason: "connection unavailable; transient messages are not replayed".to_owned(),
    }
}

// An unrepresentable deadline expires immediately rather than panicking or disabling liveness.
fn acknowledgement_deadline(timeout: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(timeout).unwrap_or(now)
}

#[expect(
    clippy::too_many_lines,
    reason = "One async stream state machine keeps cancellation and select branch lifetimes together"
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "Tokio select generates remainder operations for branch fairness; jitter uses checked arithmetic"
)]
pub async fn connect<S>(
    url: url::Url,
    mut input: S,
    options: ConnectionOptions,
) -> Result<RealtimeStream, SupabaseRealtimeError>
where
    S: Stream<Item = Item> + Unpin + Send + 'static,
{
    let minimum = Duration::from_millis(1);
    let connect_timeout = options.connect_timeout.max(minimum);
    let write_timeout = options.write_timeout.max(minimum);
    let mut socket = tokio::time::timeout(connect_timeout, connection::connect(&url))
        .await
        .map_err(|_error| SupabaseRealtimeError::ConnectTimeout)??;
    let (tx, receiver) = mpsc::channel(options.event_capacity.max(1));
    let (status_tx, status) = watch::channel(ConnectionStatus::Connecting);
    let stream_status = status_tx.clone();
    let events = async_stream::stream! {
        let minimum = Duration::from_millis(1);
        let ack_timeout = options.acknowledgement_timeout.max(minimum);
        let mut registry = Registry { joins: BTreeMap::new(), token: None, reference: 0 };
        let mut attempt = 0_u32;
        let mut jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
        'session: loop {
            status_tx.send_replace(ConnectionStatus::Joining);
            let mut replay_failed = false;
            let topics: Vec<_> = registry.joins.keys().cloned().collect();
            for topic in topics {
                if let Some(message) = registry.prepare_join(&topic, ack_timeout)
                    && let Err(error) = send(&mut socket, &message, write_timeout).await { yield Err(error); replay_failed = true; break; }
            }
            let mut heartbeat = tokio::time::interval(options.heartbeat_interval.max(minimum));
            heartbeat.reset();
            let mut pending_heartbeat: Option<(String, Instant)> = None;
            let mut timer = tokio::time::interval(minimum.max(ack_timeout.min(Duration::from_millis(100))));
            loop {
                if replay_failed { break; }
                tokio::select! {
                    incoming = input.next() => {
                        let Some(incoming) = incoming else { break 'session; };
                        let mut message = match incoming {
                            Ok(message) => message,
                            Err(error) => {
                                let terminal = matches!(error, SupabaseRealtimeError::JwtStreamClosedUnexpectedly);
                                yield Err(error);
                                if terminal { status_tx.send_replace(ConnectionStatus::Failed); return; }
                                continue;
                            }
                        };
                        registry.remember(&message, ack_timeout);
                        if matches!(message.payload, ProtocolPayload::PhxJoin(_)) {
                            status_tx.send_replace(ConnectionStatus::Joining);
                            if let Some(join) = registry.prepare_join(&message.topic, ack_timeout) { message = join; }
                        } else {
                            message.ref_field = Some(registry.next_ref());
                            message.join_ref = registry.joins.get(&message.topic).map(|join| join.reference.clone());
                            if matches!(message.payload, ProtocolPayload::PresenceInner(_)) && registry.joins.get(&message.topic).is_some_and(|join| !join.acknowledged) { continue; }
                        }
                        if let Err(error) = send(&mut socket, &message, write_timeout).await { yield Err(error); break; }
                    }
                    frame = socket.next() => {
                        let mut bytes = match frame {
                            Some(Ok(Message::Text(text))) => text.as_bytes().to_vec(),
                            Some(Ok(Message::Binary(bytes))) => bytes.to_vec(),
                            Some(Ok(Message::Close(_))) | None => break,
                            Some(Ok(_)) => continue,
                            Some(Err(error)) => { yield Err(error.into()); break; }
                        };
                        let message: ProtocolMessage = match simd_json::from_slice(&mut bytes) { Ok(message) => message, Err(error) => { yield Err(error.into()); continue; } };
                        if message.topic == "phoenix" && matches!(message.payload, ProtocolPayload::PhxReply(PhxReply::Ok(_))) {
                            if pending_heartbeat.as_ref().is_some_and(|(reference, _)| message.ref_field.as_ref() == Some(reference)) { pending_heartbeat = None; }
                            continue;
                        }
                        let mut retrack = None;
                        if let Some(join) = registry.joins.get_mut(&message.topic) {
                            if message.join_ref.as_ref().is_some_and(|reference| reference != &join.reference) { continue; }
                            match &message.payload {
                                ProtocolPayload::PhxReply(reply) if message.ref_field.as_ref() == Some(&join.reference) => {
                                    match reply {
                                        PhxReply::Ok(_) => { join.acknowledged = true; retrack.clone_from(&join.presence); }
                                        PhxReply::Error(error) => {
                                            yield Err(SupabaseRealtimeError::ChannelError { topic: message.topic.clone(), reason: error.reason.clone() });
                                            if error.reason.starts_with("RealtimeRestarting") || error.reason.starts_with("InitializingProjectConnection") { break; }
                                            status_tx.send_replace(ConnectionStatus::Failed);
                                            return;
                                        }
                                    }
                                }
                                ProtocolPayload::System(system) => {
                                    if system.status == "ok" {
                                        join.system_ready = true;
                                    } else {
                                        yield Err(SupabaseRealtimeError::ChannelError { topic: message.topic.clone(), reason: system.message.clone() });
                                        join.system_ready = false;
                                        join.system_retrying = true;
                                        status_tx.send_replace(ConnectionStatus::Joining);
                                        continue;
                                    }
                                }
                                ProtocolPayload::PresenceState(_) => { join.snapshot = true; }
                                ProtocolPayload::PresenceDiff(_) if !join.snapshot => continue,
                                ProtocolPayload::PhxError(_) | ProtocolPayload::PhxClose(_) => {
                                    yield Err(SupabaseRealtimeError::ChannelError { topic: message.topic.clone(), reason: "server closed the channel".to_owned() });
                                    break;
                                }
                                ProtocolPayload::Heartbeat(_)
                                | ProtocolPayload::AccessToken(_)
                                | ProtocolPayload::PhxJoin(_)
                                | ProtocolPayload::PhxReply(_)
                                | ProtocolPayload::Broadcast(_)
                                | ProtocolPayload::PresenceInner(_)
                                | ProtocolPayload::PresenceDiff(_)
                                | ProtocolPayload::PostgresChanges(_) => {}
                            }
                        }
                        if let Some(mut presence) = retrack {
                            presence.ref_field = Some(registry.next_ref());
                            presence.join_ref = registry.joins.get(&presence.topic).map(|join| join.reference.clone());
                            if let Err(error) = send(&mut socket, &presence, write_timeout).await { yield Err(error); break; }
                        }
                        if registry.ready() { status_tx.send_replace(ConnectionStatus::Ready); attempt = 0; }
                        yield Ok(message);
                    }
                    _ = heartbeat.tick(), if pending_heartbeat.is_none() => {
                        let reference = registry.next_ref();
                        let message = ProtocolMessage { topic: "phoenix".to_owned(), payload: ProtocolPayload::Heartbeat(crate::message::heartbeat::Heartbeat), ref_field: Some(reference.clone()), join_ref: None };
                        if let Err(error) = send(&mut socket, &message, write_timeout).await { yield Err(error); break; }
                        pending_heartbeat = Some((reference, acknowledgement_deadline(ack_timeout)));
                    }
                    _ = timer.tick() => {
                        if pending_heartbeat.as_ref().is_some_and(|(_, deadline)| Instant::now() >= *deadline) {
                            yield Err(SupabaseRealtimeError::HeartbeatTimeout); break;
                        }
                        if let Some((topic, _)) = registry.joins.iter().find(|(_, join)| (!join.acknowledged || (!join.system_ready && !join.system_retrying)) && Instant::now() >= join.deadline) {
                            yield Err(SupabaseRealtimeError::ChannelError { topic: topic.clone(), reason: "join acknowledgement timed out".to_owned() }); break;
                        }
                    }
                }
            }
            loop {
                if attempt >= options.max_retries { status_tx.send_replace(ConnectionStatus::Failed); yield Err(SupabaseRealtimeError::ReconnectExhausted); return; }
                attempt = attempt.saturating_add(1);
                jitter ^= jitter << 13_u32; jitter ^= jitter >> 17_u32; jitter ^= jitter << 5_u32;
                let ceiling = options.initial_backoff.max(minimum).saturating_mul(1_u32 << attempt.saturating_sub(1).min(30)).min(options.max_backoff.max(minimum));
                // Scale in integer thousandths without overflowing Duration at large bounds.
                let fraction = jitter.checked_rem(501).unwrap_or_default().saturating_add(500);
                let delay = ceiling.checked_div(1000).unwrap_or_default().saturating_mul(fraction);
                status_tx.send_replace(ConnectionStatus::Reconnecting { attempt, delay });
                let sleep = tokio::time::sleep(delay);
                tokio::pin!(sleep);
                loop {
                    tokio::select! {
                        () = &mut sleep => break,
                        incoming = input.next() => {
                            match incoming {
                                Some(Ok(message)) => {
                                    registry.remember(&message, ack_timeout);
                                    if !matches!(message.payload, ProtocolPayload::PhxJoin(_) | ProtocolPayload::AccessToken(_) | ProtocolPayload::PresenceInner(_) | ProtocolPayload::PhxClose(_)) { yield Err(unavailable(message.topic)); }
                                }
                                Some(Err(error)) => {
                                    let terminal = matches!(error, SupabaseRealtimeError::JwtStreamClosedUnexpectedly);
                                    yield Err(error);
                                    if terminal { status_tx.send_replace(ConnectionStatus::Failed); return; }
                                }
                                None => break 'session,
                            }
                        }
                    }
                }
                let connection = tokio::time::timeout(connect_timeout, connection::connect(&url));
                tokio::pin!(connection);
                loop {
                    tokio::select! {
                        result = &mut connection => {
                            match result {
                                Ok(Ok(next)) => { socket = next; continue 'session; }
                                Ok(Err(error)) => { yield Err(error); break; }
                                Err(_) => { yield Err(SupabaseRealtimeError::ConnectTimeout); break; }
                            }
                        }
                        incoming = input.next() => {
                            match incoming {
                                Some(Ok(message)) => {
                                    registry.remember(&message, ack_timeout);
                                    if !matches!(message.payload, ProtocolPayload::PhxJoin(_) | ProtocolPayload::AccessToken(_) | ProtocolPayload::PresenceInner(_) | ProtocolPayload::PhxClose(_)) { yield Err(unavailable(message.topic)); }
                                }
                                Some(Err(error)) => {
                                    let terminal = matches!(error, SupabaseRealtimeError::JwtStreamClosedUnexpectedly);
                                    yield Err(error);
                                    if terminal { status_tx.send_replace(ConnectionStatus::Failed); return; }
                                }
                                None => break 'session,
                            }
                        }
                    }
                }
            }
        }
        status_tx.send_replace(ConnectionStatus::Closed);
    };
    let task = tokio::spawn(async move {
        tokio::pin!(events);
        while let Some(event) = events.next().await {
            if tx.send(event).await.is_err() {
                break;
            }
        }
    });
    Ok(RealtimeStream {
        receiver,
        task,
        status,
        status_tx: stream_status,
    })
}
