# supabase-rp

[![crates.io](https://img.shields.io/crates/v/supabase-rp.svg)](https://crates.io/crates/supabase-rp) [![docs.rs](https://docs.rs/supabase-rp/badge.svg)](https://docs.rs/supabase-rp)

One entry point for [Supabase](https://supabase.com) from Rust.
`Client` holds your project URL, your API key, and one shared HTTP connection pool.
It gives you REST, auth, storage, edge functions, GraphQL, and realtime configuration.

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
| `graphql`   | no      | `Client::graphql` with typed data and partial errors    |
| `rustls`    | yes     | rustls for HTTP and WebSocket connections              |
| `native-tls` | no     | Native TLS when default features are disabled          |
| `typed`     | no      | Re-exports `rp-supabase-client` as `supabase_rp::typed` (schema runtime only) |
| `typed-client` | no   | `typed` plus the client API of `rp-supabase-client` (enables `serde_json/arbitrary_precision`) |
| `test-util` | no      | `rp-postgrest/test-util`, for example `postgrest::Error::from_response` in tests |
| `full`      | no      | All services and typed-client; does not select a TLS backend |

Each enabled crate is re-exported as a module (see [Module map](#module-map)).
You need only this one dependency.

The default HTTP client of `Client::new` follows up to 10 redirects, and only to the same origin (scheme, host, port), so credentials never reach another host. `Client::new_with_client` uses your client and its redirect policy unchanged.

HTTP reads retry explicit 429 responses twice by default. The policy honors `Retry-After`
without shortening server delays. Mutations, streaming request bodies, transport failures,
and other HTTP statuses are not replayed by default. Use `Client::with_retry_policy`
to change this policy. `RetryPolicy::none()` disables it.

For native TLS, use `default-features = false, features = ["full", "native-tls"]`.
rustls takes precedence when both backends are enabled. A supplied HTTP client keeps its own TLS policy.

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

## String-based writes

Use the same builder for reads and writes. `_json` methods serialize your serde types.
Filters apply to updates and deletes. The examples request the affected rows.

```rust,no_run
# #[cfg(feature = "rest")]
# async fn run(client: supabase_rp::Client) -> Result<(), Box<dyn std::error::Error>> {
#[derive(serde::Serialize)]
struct WriteTodo<'a> { title: &'a str }
#[derive(serde::Deserialize)]
struct Todo { id: i64, title: String }

let created: Vec<Todo> = client.from("todos")
    .insert_json(&WriteTodo { title: "before" }).select("id,title").fetch().await?;
let id = created.first().ok_or("insert returned no rows")?.id.to_string();
let updated: Vec<Todo> = client.from("todos")
    .update_json(&WriteTodo { title: "after" }).eq("id", &id)
    .select("id,title").fetch().await?;
let deleted: Vec<Todo> = client.from("todos").delete().eq("id", &id)
    .select("id,title").fetch().await?;
# Ok(())
# }
```

## Sign in and act as the user

Sign in, then call `with_access_token`.
The new client sends the user token to REST, auth, storage, functions, and GraphQL, so row level security applies.
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
Auth also supports anonymous sessions, OAuth PKCE, memory and file session stores,
admin user management, and TOTP or phone MFA. See the [auth guide](../supabase-auth/README.md).
After MFA verification, use the returned session's access token to construct the upgraded user client.
Session stores and AAL claim inspection do not replace server-side authorization.

`Client::admin(service_role_key)` creates an elevated client with explicit server-only credentials.
It shares the HTTP pool but does not change the user client. Never send service-role keys to untrusted clients.


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

## GraphQL

Requires the `graphql` feature. Execution errors can accompany partial data, even with HTTP 200.
The response preserves both.

```rust,no_run
# #[cfg(feature = "graphql")]
# async fn run(client: supabase_rp::Client) -> Result<(), Box<dyn std::error::Error>> {
#[derive(Debug, serde::Deserialize)]
struct Data {
    #[serde(rename = "__typename")]
    kind: String,
}
let response = client.graphql().query("{ __typename }").fetch::<Data>().await?;
println!("data: {:?}; errors: {:?}", response.data, response.errors);
# Ok(())
# }
```

## Realtime

Requires the `realtime` feature. `realtime_config` returns the configuration that
`RealtimeConnection::db_changes`, `presence`, and `broadcast` take. Realtime signs in on its
own with `LoginCredentials`. Sign-in and refresh retries use the returned auth configuration.
WebSocket recovery has separate `ConnectionOptions`. It replays channel joins with the latest
token and restores tracked presence. Messages sent during an outage are not replayed.

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
| `supabase_rp::graphql`  | `rp-supabase-graphql`  | `graphql`   |
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
| [rp-supabase-graphql](https://docs.rs/rp-supabase-graphql)             | GraphQL requests and partial responses      |
| [rp-supabase-http](https://docs.rs/rp-supabase-http)                   | Shared TLS and rate-limit retry policy      |
| [rp-supabase-realtime](https://docs.rs/rp-supabase-realtime)           | Realtime changes, broadcast, presence      |
| [rp-supabase-client](https://docs.rs/rp-supabase-client)               | Typed runtime for generated schemas        |
| [rp-supabase-codegen](https://docs.rs/rp-supabase-codegen)             | Rust types generated from your database    |
| [rp-supabase-mock](https://docs.rs/rp-supabase-mock)                   | Mock Supabase server for tests             |
