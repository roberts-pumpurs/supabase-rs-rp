# Supabase Rust utilities

[![Tests](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/test.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/test.yaml) [![Checks](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/check.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/check.yaml) [![Audit](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/audit.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/audit.yaml) [![Deny](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/deny.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/deny.yaml) [![Docs](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/doc.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/doc.yaml) [![Unused Dependencies](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/unused-deps.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/unused-deps.yaml) [![Conventional PR](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/conventional-pr.yaml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/conventional-pr.yaml) [![Release](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/release-plz.yml/badge.svg)](https://github.com/roberts-pumpurs/supabase-rs-rp/actions/workflows/release-plz.yml) [![crates.io](https://img.shields.io/crates/v/supabase-rp.svg)](https://crates.io/crates/supabase-rp) [![docs.rs](https://img.shields.io/docsrs/supabase-rp)](https://docs.rs/supabase-rp)

Rust crates for [Supabase](https://supabase.com): database (PostgREST), auth, storage, edge functions, and realtime.
Use them in async Rust services, CLIs, and tests that talk to a Supabase project.
Start with the `supabase-rp` crate and plain queries. Add generated, compile-time checked types later if you want them.
All crates require Rust 1.88 or later.

## Quickstart

Add the umbrella crate:

```sh
cargo add supabase-rp --features full
cargo add serde --features derive
cargo add tokio --features macros,rt-multi-thread
```

Query a table. Pass the project base URL and the project API key.

```rust
use supabase_rp::Client;

#[derive(Debug, serde::Deserialize)]
struct Todo {
    id: i64,
    title: String,
}

let client = Client::new("https://abc.supabase.co/", "your-anon-key")?;
let todos = client.from("todos").select("id,title").fetch::<Vec<Todo>>().await?;
println!("{todos:?}");
```

### Act as a user

Sign in, then call `with_access_token`. The new client sends the user token, so row level security applies.

```rust
use supabase_rp::auth::types::LoginCredentials;

let login = LoginCredentials::builder()
    .email("user@example.com".to_owned())
    .password("password".to_owned())
    .build();
let session = client.auth().sign_in_with_password(&login).await?;
let token = session.access_token.ok_or("sign-in returned no access token")?;
let user_client = client.with_access_token(&token)?;
let me = user_client.auth().get_user().await?;
```

The [supabase-rp README](./crates/supabase-rp/README.md) shows storage, edge functions, and realtime.

## When you want compile-time checked queries

The untyped path checks column names at runtime, on the server.
The typed path moves these checks to compile time.
[rp-supabase-codegen](./crates/supabase-codegen/README.md) reads a schema snapshot in `build.rs` and generates Rust types for your tables, columns, and relations.
[rp-supabase-client](./crates/supabase-client/README.md) runs queries on these types: `select!` builds a projection, and filters and ordering take typed columns.
The [codegen example](./crates/supabase-codegen-example/README.md) builds from a committed snapshot without a database.
Enable the `typed` feature of `supabase-rp` to get the `rp-supabase-client` schema runtime as `supabase_rp::typed`. Enable `typed-client` for its client API too.

## Crates

| Crate | Version | What it does | Use it directly when |
|-------|---------|--------------|----------------------|
| [supabase-rp](./crates/supabase-rp/README.md) | [![crates.io](https://img.shields.io/crates/v/supabase-rp.svg)](https://crates.io/crates/supabase-rp) | One `Client` for REST, auth, storage, functions, and realtime configuration | You start a new project. This is the default entry point. |
| [rp-postgrest](./crates/postgrest/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-postgrest.svg)](https://crates.io/crates/rp-postgrest) | PostgREST query builder with checked execution and typed decoding | You talk to a PostgREST server without the rest of Supabase. |
| [rp-postgrest-error](./crates/postgrest-error/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-postgrest-error.svg)](https://crates.io/crates/rp-postgrest-error) | PostgREST and PostgreSQL error model | You map database errors in your own code. |
| [rp-supabase-auth](./crates/supabase-auth/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-auth.svg)](https://crates.io/crates/rp-supabase-auth) | Supabase Auth API client and token refresh streams | You need auth only, or the full request types (MFA, admin). |
| [rp-supabase-storage](./crates/supabase-storage/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-storage.svg)](https://crates.io/crates/rp-supabase-storage) | Storage buckets, objects, and signed URLs | You need storage only. |
| [rp-supabase-functions](./crates/supabase-functions/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-functions.svg)](https://crates.io/crates/rp-supabase-functions) | Edge function invocation | You need functions only. |
| [rp-supabase-realtime](./crates/supabase-realtime/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-realtime.svg)](https://crates.io/crates/rp-supabase-realtime) | Realtime database changes, broadcast, and presence | You need realtime only. |
| [rp-supabase-client](./crates/supabase-client/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-client.svg)](https://crates.io/crates/rp-supabase-client) | Typed query runtime for generated schemas | You use generated types. |
| [rp-supabase-client-macros](./crates/supabase-client-macros) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-client-macros.svg)](https://crates.io/crates/rp-supabase-client-macros) | Procedural macros for rp-supabase-client | Never. rp-supabase-client re-exports them. |
| [rp-supabase-codegen](./crates/supabase-codegen/README.md) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-codegen.svg)](https://crates.io/crates/rp-supabase-codegen) | Generates Rust schema types in `build.rs` | You add typed queries (build dependency). |
| [rp-supabase-mock](./crates/supabase-mock) | [![crates.io](https://img.shields.io/crates/v/rp-supabase-mock.svg)](https://crates.io/crates/rp-supabase-mock) | Mock Supabase server for tests | You test code that calls Supabase. |

## Feature coverage

| Supabase service | Supported | Limits |
|------------------|-----------|--------|
| Database (PostgREST) | Queries, filters, ordering, pagination, counts, inserts, updates, upserts, deletes, RPC | No GraphQL. |
| Auth | Password, OTP, sign up, refresh, user update, sign out. Request types for MFA and admin endpoints. | No session storage. No convenience methods for OAuth redirects or PKCE code exchange. |
| Realtime | Database changes (also typed), broadcast, presence | The websocket does not reconnect; the stream ends when the server closes it. One channel topic per connection. Sign in with email or phone and password only. |
| Storage | Bucket CRUD, object upload, download, move, copy, remove, list, signed and public URLs | No resumable (TUS) uploads. No image transformations. No S3 protocol. |
| Edge Functions | Invoke with JSON or raw bodies, custom headers and methods | No streamed request bodies. No multipart helpers. |
| Typed codegen | Tables, views, columns, relations, RPC, from a snapshot or a live database | Regenerate after you upgrade `rp-supabase-codegen`. |

Read each crate README for the full list of limits.
See [docs/comparison.md](./docs/comparison.md) for a comparison with other Rust Supabase clients.

## Examples

The [examples directory](./examples/README.md) has runnable programs for auth, realtime (database changes, broadcast, presence), JWT refresh, and database queries.
The [codegen example](./crates/supabase-codegen-example/README.md) shows build-script schema bindings with offline generation and live CRUD and RPC checks.

## Development guide

1. Install the pinned Rust 1.99.0 toolchain with `rustup show`. Development and CI use this stable release. The published crates support Rust 1.88.
2. Run `cargo fmt --all --check`.
3. Run `cargo clippy --workspace --all-features --all-targets --locked -- -D warnings`.
4. Run `cargo test --workspace --all-features --all-targets --locked`.
5. Run `cargo test --workspace --all-features --doc --locked` for documentation examples.

`cargo xtask check` runs Clippy and formatting checks. `cargo xtask fmt` also applies compiler and Clippy fixes.

## Supabase instance for local development

1. [Install Supabase CLI](https://supabase.com/docs/guides/cli/getting-started)
2. Run `supabase start` to run local supabase instance
