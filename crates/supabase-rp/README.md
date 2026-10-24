# supabase-rp

[![crates.io](https://img.shields.io/crates/v/supabase-rp.svg)](https://crates.io/crates/supabase-rp) [![docs.rs](https://docs.rs/supabase-rp/badge.svg)](https://docs.rs/supabase-rp)

One entry point for [Supabase](https://supabase.com) from Rust.
`Client` holds your project URL, your API key, and one shared HTTP connection pool.
It gives you REST, auth, storage, edge functions, and realtime configuration.

## Install

```toml
[dependencies]
supabase-rp = { version = "0.6", features = ["full"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

| Feature     | Default | Enables                                                 |
|-------------|---------|---------------------------------------------------------|
| `rest`      | yes     | `Client::rest`, `Client::from` (PostgREST)              |
| `auth`      | yes     | `Client::auth` (Supabase Auth)                          |
| `storage`   | no      | `Client::storage` (buckets, objects, signed URLs)       |
| `functions` | no      | `Client::functions` (edge functions)                    |
| `realtime`  | no      | `Client::realtime_config` (websocket channels)          |
| `typed`     | no      | Re-exports `rp-supabase-client` as `supabase_rp::typed` (schema runtime only) |
| `typed-client` | no   | `typed` plus the client API of `rp-supabase-client` (enables `serde_json/arbitrary_precision`) |
| `test-util` | no      | `rp-postgrest/test-util`, for example `postgrest::Error::from_response` in tests |
| `full`      | no      | All of the above except `test-util`                     |

Each enabled crate is re-exported as a module (see [Module map](#module-map)).
You need only this one dependency.

The default HTTP client of `Client::new` follows up to 10 redirects, and only to the same origin (scheme, host, port), so credentials never reach another host. `Client::new_with_client` uses your client and its redirect policy unchanged.

## Quickstart

Pass the project root URL, for example `https://abc.supabase.co/`, and the project API key.
A URL with a path, such as `https://gateway.example/supabase`, is rejected.

```rust,no_run
use supabase_rp::Client;

#[derive(Debug, serde::Deserialize)]
struct Todo {
    id: i64,
    title: String,
}

# #[cfg(feature = "rest")]
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::new("https://abc.supabase.co/", "your-anon-key")?;
let todos = client.from("todos").select("id,title").fetch::<Vec<Todo>>().await?;
println!("{todos:?}");
# Ok(())
# }
```

## Sign in and act as the user

Sign in, then call `with_access_token`.
The new client sends the user token to REST, auth, storage, and functions, so row level security applies.
The `apikey` header keeps the project key. The connection pool is shared.

```rust,no_run
# #[cfg(all(feature = "rest", feature = "auth"))]
# async fn run(client: supabase_rp::Client) -> Result<(), Box<dyn std::error::Error>> {
use supabase_rp::auth::types::LoginCredentials;

let login = LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());
let session = client.auth().sign_in_with_password(&login).await?;
let token = session.access_token.ok_or("sign-in returned no access token")?;
let user_client = client.with_access_token(&token)?;

let me = user_client.auth().get_user().await?;
println!("{me:?}");
user_client.auth().sign_out().await?;
# Ok(())
# }
```

## Storage

Requires the `storage` feature.

```rust,no_run
# #[cfg(feature = "storage")]
# async fn run(client: supabase_rp::Client) -> Result<(), Box<dyn std::error::Error>> {
use supabase_rp::storage::FileOptions;

let avatars = client.storage().from("avatars");
let options = FileOptions {
    content_type: Some("image/png".to_owned()),
    ..FileOptions::default()
};
avatars.upload("user-1/avatar.png", vec![0_u8; 16], &options).await?;
let url = avatars.create_signed_url("user-1/avatar.png", 60).await?;
println!("{url}");
# Ok(())
# }
```

### Separate storage URL

A custom domain can proxy REST and auth but not storage.
Call `with_storage_url` with the storage project root URL. Storage still joins `storage/v1`.
REST, auth, and functions keep the URL from `Client::new`.
An access token from `with_access_token` stays, in either call order.

```rust,no_run
# #[cfg(feature = "storage")]
# fn run() -> Result<(), supabase_rp::Error> {
let client = supabase_rp::Client::new("https://api.example.com/", "your-anon-key")?
    .with_storage_url("https://abc.supabase.co/")?;
# Ok(())
# }
```

## Edge functions

Requires the `functions` feature.

```rust,no_run
# #[cfg(feature = "functions")]
# async fn run(client: supabase_rp::Client) -> Result<(), Box<dyn std::error::Error>> {
#[derive(serde::Serialize)]
struct Input<'a> {
    name: &'a str,
}

#[derive(Debug, serde::Deserialize)]
struct Output {
    message: String,
}

let reply: Output = client
    .functions()
    .invoke("hello")
    .json(&Input { name: "world" })
    .fetch()
    .await?;
println!("{}", reply.message);
# Ok(())
# }
```

## Realtime

Requires the `realtime` feature. `realtime_config` returns the configuration that
`RealtimeConnection::db_changes`, `presence`, and `broadcast` take. Realtime signs in on its
own with `LoginCredentials`. If sign-in or a token refresh fails, it retries up to 5 times,
3 seconds apart. The websocket itself does not reconnect. Change the returned fields to
override the retry settings.

```rust,no_run
# #[cfg(feature = "realtime")]
# async fn run(
#     client: supabase_rp::Client,
#     login: supabase_rp::realtime::rp_supabase_auth::types::LoginCredentials,
# ) -> Result<(), Box<dyn std::error::Error>> {
use supabase_rp::realtime::futures::StreamExt as _;
use supabase_rp::realtime::message::phx_join::PostgresChanges;
use supabase_rp::realtime::realtime::{RealtimeConnection, typed_changes};

#[derive(Debug, serde::Deserialize)]
struct Todo {
    id: i64,
    title: String,
}

let (stream, mut channel) = RealtimeConnection::db_changes(client.realtime_config())
    .connect(login)
    .await?;
channel.subscribe_to_changes(vec![PostgresChanges::table("todos")]).await?;
let mut changes = std::pin::pin!(typed_changes::<Todo>(stream));
while let Some(change) = changes.next().await {
    println!("{:?}", change?);
}
# Ok(())
# }
```

See [rp-supabase-realtime](https://docs.rs/rp-supabase-realtime) for broadcast and presence.

## Typed queries from your schema

The `typed` feature re-exports [rp-supabase-client](https://docs.rs/rp-supabase-client).
Generate Rust types for your tables with [rp-supabase-codegen](https://docs.rs/rp-supabase-codegen),
then run typed queries against `client.rest()`. It is the same `Postgrest` the typed runtime takes.

`typed` enables only the schema runtime. Set the codegen `runtime_path` to
`::supabase_rp::typed::schema`, and you need no direct `rp-supabase-client` dependency.
Enable `typed-client` for the client API (authentication helpers and response decoding).

## Module map

| Path | Crate | Feature |
|------|-------|---------|
| `supabase_rp::postgrest` | `rp-postgrest`         | `rest`      |
| `supabase_rp::auth`      | `rp-supabase-auth`     | `auth`      |
| `supabase_rp::storage`   | `rp-supabase-storage`  | `storage`   |
| `supabase_rp::functions` | `rp-supabase-functions` | `functions` |
| `supabase_rp::realtime`  | `rp-supabase-realtime` | `realtime`  |
| `supabase_rp::typed`     | `rp-supabase-client`   | `typed`     |

The module `supabase_rp::auth` holds the auth types. The method `Client::auth()` returns the auth client.

## Underlying crates

| Crate                                                                  | Purpose                                    |
|------------------------------------------------------------------------|--------------------------------------------|
| [rp-postgrest](https://docs.rs/rp-postgrest)                           | PostgREST query builder and decoding       |
| [rp-postgrest-error](https://docs.rs/rp-postgrest-error)               | Typed PostgREST error codes                |
| [rp-supabase-auth](https://docs.rs/rp-supabase-auth)                   | Supabase Auth API and token refresh        |
| [rp-supabase-storage](https://docs.rs/rp-supabase-storage)             | Storage buckets, objects, signed URLs      |
| [rp-supabase-functions](https://docs.rs/rp-supabase-functions)         | Edge function invocation                   |
| [rp-supabase-realtime](https://docs.rs/rp-supabase-realtime)           | Realtime changes, broadcast, presence      |
| [rp-supabase-client](https://docs.rs/rp-supabase-client)               | Typed runtime for generated schemas        |
| [rp-supabase-codegen](https://docs.rs/rp-supabase-codegen)             | Rust types generated from your database    |
| [rp-supabase-mock](https://docs.rs/rp-supabase-mock)                   | Mock Supabase server for tests             |
