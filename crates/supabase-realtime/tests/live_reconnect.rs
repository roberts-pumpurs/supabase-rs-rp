//! Run explicitly with the local fixture and an external Realtime container restart.
#![expect(clippy::unwrap_used, reason = "Opt-in acceptance assertions")]
#![expect(
    clippy::panic,
    reason = "Missing prerequisites and unexpected protocol events fail explicitly"
)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Live integration test crate"
)]

use core::time::Duration;
use futures::{FutureExt as _, SinkExt as _, StreamExt as _};
use rp_supabase_realtime::{
    ConnectionOptions, ConnectionStatus,
    message::{ProtocolMessage, ProtocolPayload, postgres_changes::PostgresChange},
    realtime::RealtimeBaseConnection,
};
use serde::Deserialize;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_error| panic!("missing required {name}"))
}
fn decode(text: &str) -> ProtocolMessage {
    simd_json::from_slice(&mut text.as_bytes().to_vec()).unwrap()
}

#[derive(Deserialize)]
struct Row {
    message: String,
}
#[derive(Default)]
struct Observations {
    broadcast: bool,
    presence: bool,
    database: bool,
}
impl Observations {
    fn observe(&mut self, payload: ProtocolPayload, event: &str, presence_key: &str) {
        match payload {
            ProtocolPayload::Broadcast(message) => self.broadcast |= message.event == event,
            ProtocolPayload::PresenceState(state) => {
                self.presence |= state.0.contains_key(presence_key);
            }
            ProtocolPayload::PresenceDiff(diff) => {
                self.presence |= diff.joins.contains_key(presence_key);
            }
            ProtocolPayload::PostgresChanges(change) => {
                match change.data.into_change::<Row>().unwrap() {
                    PostgresChange::Insert { record, metadata } => {
                        assert_eq!(
                            metadata.table, "product_events",
                            "CDC must come from the subscribed table"
                        );
                        self.database |= record.message == event;
                    }
                    PostgresChange::Update { .. } | PostgresChange::Delete { .. } => {
                        panic!("INSERT subscription received another change kind")
                    }
                }
            }
            ProtocolPayload::Heartbeat(_)
            | ProtocolPayload::AccessToken(_)
            | ProtocolPayload::PhxJoin(_)
            | ProtocolPayload::PhxReply(_)
            | ProtocolPayload::PresenceInner(_)
            | ProtocolPayload::PhxClose(_)
            | ProtocolPayload::System(_)
            | ProtocolPayload::PhxError(_) => {}
        }
    }
    const fn complete(&self) -> bool {
        self.broadcast && self.presence && self.database
    }
}

#[tokio::test]
#[ignore = "requires local Supabase and parent-driven Realtime container restart"]
#[expect(
    clippy::print_stdout,
    reason = "The restart harness consumes the readiness marker"
)]
async fn broadcast_presence_and_database_recover_after_restart() {
    let base = url::Url::parse(&required("SUPABASE_URL")).unwrap();
    let key = required("SUPABASE_ANON_KEY");
    let token = required("SUPABASE_ACCESS_TOKEN");
    let presence_key = format!("recovery-{}", std::process::id());
    let mut url = base.join("realtime/v1/websocket").unwrap();
    url.query_pairs_mut()
        .append_pair("apikey", &key)
        .append_pair("vsn", "1.0.0");
    let (mut tx, rx) = futures::channel::mpsc::channel(8);
    let mut stream = RealtimeBaseConnection::new(url)
        .with_options(ConnectionOptions {
            max_retries: 60,
            max_backoff: Duration::from_secs(2),
            ..ConnectionOptions::default()
        })
        .connect(rx)
        .await
        .unwrap();
    let mut join = decode(
        r#"{"topic":"realtime:product-recovery","event":"phx_join","payload":{"config":{"private":false,"broadcast":{"self":true,"ack":false},"presence":{"enabled":true,"key":""},"postgres_changes":[{"event":"INSERT","schema":"public","table":"product_events"}]}},"ref":null,"join_ref":null}"#,
    );
    if let ProtocolPayload::PhxJoin(config) = &mut join.payload {
        config.config.presence.key.clone_from(&presence_key);
    }
    join.set_access_token(&token);
    tx.send(Ok(join)).await.unwrap();
    let mut status = stream.status();
    let http = reqwest::Client::new();
    #[expect(
        clippy::integer_division_remainder_used,
        reason = "Tokio select uses modulo for fair branch scheduling"
    )]
    tokio::time::timeout(Duration::from_secs(180), async {
        for phase in 0_u8..2 {
            let event = format!("recovery-{phase}");
            let mut observed = Observations::default();
            while *status.borrow() != ConnectionStatus::Ready {
                tokio::select! {
                    changed = status.changed() => { changed.unwrap(); }
                    item = stream.next() => match item {
                        Some(Ok(message)) => observed.observe(message.payload, &event, &presence_key),
                        Some(Err(error)) if phase == 0 => panic!("initial readiness failed: {error}"),
                        Some(Err(_error)) => {}, // A deliberate restart produces transport errors before rejoin.
                        None => panic!("channel ended before Ready"),
                    }
                }
            }
            while let Some(Some(item)) = stream.next().now_or_never() {
                if let Ok(message) = item { observed.observe(message.payload, &event, &presence_key); }
            }
            // Track once. The driver, not this test, must restore presence after the restart.
            if phase == 0 {
                tx.send(Ok(decode(r#"{"topic":"realtime:product-recovery","event":"presence","payload":{"type":"presence","event":"track","payload":{"online":true}},"ref":null,"join_ref":null}"#))).await.unwrap();
            }
            tx.send(Ok(decode(&format!(r#"{{"topic":"realtime:product-recovery","event":"broadcast","payload":{{"type":"broadcast","event":"{event}","payload":{{}}}},"ref":null,"join_ref":null}}"#)))).await.unwrap();
            http.post(base.join("rest/v1/product_events").unwrap()).header("apikey", &key).bearer_auth(&token).header("Content-Type", "application/json").body(format!(r#"{{"message":"{event}"}}"#)).send().await.unwrap().error_for_status().unwrap();
            while !observed.complete() {
                let message = stream.next().await.unwrap().unwrap();
                observed.observe(message.payload, &event, &presence_key);
            }
            if phase == 0 {
                println!("REALTIME_READY_RESTART");
                loop {
                    status.changed().await.unwrap();
                    if matches!(*status.borrow(), ConnectionStatus::Reconnecting { .. }) { break; }
                }
            }
        }
    }).await.unwrap();
    stream.shutdown().await;
}
