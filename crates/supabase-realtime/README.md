# rp-supabase-realtime

[![crates.io](https://img.shields.io/crates/v/rp-supabase-realtime.svg)](https://crates.io/crates/rp-supabase-realtime) [![docs.rs](https://docs.rs/rp-supabase-realtime/badge.svg)](https://docs.rs/rp-supabase-realtime)

A Rust client for [Supabase Realtime](https://supabase.com/docs/guides/realtime).
It connects over a websocket, signs in with Supabase Auth, and gives you three channel types:

- **Postgres changes**: receive inserts, updates, and deletes on your tables.
- **Broadcast**: send and receive messages between clients.
- **Presence**: share and track the state of connected clients.

## Install

```toml
[dependencies]
rp-supabase-realtime = "0.11"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Use the version on [crates.io](https://crates.io/crates/rp-supabase-realtime).
The crate re-exports `futures`, `url`, and `rp_supabase_auth`.

## Connect

Every channel needs a `SupabaseAuthConfig` and the credentials of a user.
The project URL is the base URL of your project, for example `https://abc.supabase.co/`.

```rust,no_run
use core::time::Duration;

use rp_supabase_realtime::rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_realtime::rp_supabase_auth::types::LoginCredentials;

# fn run() -> Result<(), Box<dyn std::error::Error>> {
let config = SupabaseAuthConfig {
    url: "https://abc.supabase.co/".parse()?,
    api_key: "your-anon-key".to_owned(),
    max_reconnect_attempts: 5,
    reconnect_interval: Duration::from_secs(3),
};
let login = LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());
# Ok(())
# }
```

## Postgres changes with typed rows

Subscribe to a table, then pass the output stream to `typed_changes`.
It decodes each row into your type and drops protocol messages such as heartbeats and replies.

```rust,no_run
# use core::time::Duration;
# use rp_supabase_realtime::rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
# use rp_supabase_realtime::rp_supabase_auth::types::LoginCredentials;
use rp_supabase_realtime::futures::StreamExt as _;
use rp_supabase_realtime::message::phx_join::{PostgresChangeEvent, PostgresChanges};
use rp_supabase_realtime::message::postgres_changes::PostgresChange;
use rp_supabase_realtime::realtime::{RealtimeConnection, typed_changes};

#[derive(Debug, serde::Deserialize)]
struct Message {
    id: i64,
    body: String,
}

# async fn run(config: SupabaseAuthConfig, login: LoginCredentials) -> Result<(), Box<dyn std::error::Error>> {
let (stream, mut client) = RealtimeConnection::db_changes(config).connect(login).await?;

client
    .subscribe_to_changes(vec![
        PostgresChanges::table("messages"),
        PostgresChanges::table("audit_log")
            .schema("private")
            .event(PostgresChangeEvent::Insert)
            .filter("user_id=eq.42"),
    ])
    .await?;

let mut changes = std::pin::pin!(typed_changes::<Message>(stream));
while let Some(change) = changes.next().await {
    match change? {
        PostgresChange::Insert { record, metadata } => {
            println!("insert into {}: {record:?}", metadata.table);
        }
        PostgresChange::Update { record, old_record, .. } => {
            println!("update {old_record:?} -> {record:?}");
        }
        PostgresChange::Delete { old_record, .. } => println!("delete {old_record:?}"),
    }
}
# Ok(())
# }
```

All subscribed tables share one stream, so `T` must decode every row you subscribe to.
Use `rp_supabase_realtime::simd_json::OwnedValue` as `T` to receive untyped rows.

`old_record` holds only the primary key columns.
To receive the full previous row, run `ALTER TABLE messages REPLICA IDENTITY FULL;`.
Row level security applies to inserts and updates: the signed-in user receives only the rows it can select.
Deletes are an exception. Realtime cannot apply row level security to deletes, so every subscriber receives every delete.
With row level security enabled, a delete's `old_record` holds only the primary key columns, even with `REPLICA IDENTITY FULL`.
Enable Realtime for the table in the Supabase dashboard or add it to the `supabase_realtime` publication.

## Broadcast

```rust,no_run
# use rp_supabase_realtime::rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
# use rp_supabase_realtime::rp_supabase_auth::types::LoginCredentials;
use rp_supabase_realtime::futures::StreamExt as _;
use rp_supabase_realtime::message::broadcast::Broadcast;
use rp_supabase_realtime::message::phx_join::BroadcastConfig;
use rp_supabase_realtime::realtime::RealtimeConnection;

# async fn run(config: SupabaseAuthConfig, login: LoginCredentials) -> Result<(), Box<dyn std::error::Error>> {
let (mut stream, mut client) = RealtimeConnection::broadcast(config, "room-1")
    .connect(login)
    .await?;

// `self_item: true` echoes your own messages back to you.
client.join(BroadcastConfig { self_item: true, ack: true }).await?;
client
    .broadcast(Broadcast {
        r#type: "broadcast".to_owned(),
        event: "cursor".to_owned(),
        payload: rp_supabase_realtime::simd_json::json!({ "x": 10, "y": 20 }),
    })
    .await?;

while let Some(msg) = stream.next().await {
    println!("{:?}", msg?.payload);
}
# Ok(())
# }
```

## Presence

`connect_with_state_tracking` keeps the presence state for you.
It yields the full state after each `presence_state` or `presence_diff` message.
`payload` holds every key you track, including `name`. Only `phx_ref` is reserved.

```rust,no_run
# use rp_supabase_realtime::rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
# use rp_supabase_realtime::rp_supabase_auth::types::LoginCredentials;
use rp_supabase_realtime::futures::StreamExt as _;
use rp_supabase_realtime::futures::future::Either;
use rp_supabase_realtime::realtime::RealtimeConnection;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct UserState {
    name: String,
}

# async fn run(config: SupabaseAuthConfig, login: LoginCredentials) -> Result<(), Box<dyn std::error::Error>> {
let (stream, mut client) = RealtimeConnection::presence(config, "lobby")
    .connect_with_state_tracking::<UserState>(login)
    .await?;

client.join(Some("user-1".to_owned())).await?;
client.track(&UserState { name: "Ada".to_owned() }).await?;

let mut stream = std::pin::pin!(stream);
while let Some(msg) = stream.next().await {
    if let Either::Left(state) = msg? {
        println!("{} clients online", state.metas.len());
    }
}
# Ok(())
# }
```

## Token refresh

`connect` signs in through `rp_supabase_auth::jwt_stream::JwtStream`.
The stream refreshes the access token when half of its lifetime has passed.
The connection sends each new token to the server, and later messages carry it.
If sign in fails, the stream retries `max_reconnect_attempts` times and waits `reconnect_interval` between attempts.

The connection sends a heartbeat every 20 seconds.

## Limits

- Only email or phone and password sign in is supported. You cannot pass an existing access token.
- The client does not reconnect the websocket. When the server closes the connection, the stream ends.
  Create a new connection to continue.
- A connection holds one channel topic.
- `subscribe_to_changes` and `join` do not wait for the server's `phx_reply`. `typed_changes` turns a
  failed join into `SupabaseRealtimeError::ChannelError`. With the raw stream, read the replies yourself.
