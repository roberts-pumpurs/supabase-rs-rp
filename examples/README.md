# Supabase utils examples

These applications use the service crates in this workspace directly: `rp-postgrest` 3.2,
`rp-supabase-client` 0.11, and the auth and realtime crates at 0.9.
For a new project, start with the umbrella crate [`supabase-rp`](../crates/supabase-rp/README.md) 0.1.
It wraps these crates behind one `Client`. Its README has short examples for REST, auth,
storage, edge functions, and realtime.

## Local Supabase prerequisites

- Rust toolchain installed
- Supabase CLI installed
- A running Supabase instance (the examples will try to connect to a local instance at `./supabase`)

```bash
cd supabase
supabase start
```

## Running the examples

All examples can be run using Cargo. For example:

```bash
cargo run --bin auth-example
```

## Available examples

| Example | File | Description | Use Cases |
|---------|------|-------------|-----------|
| Auth Example | [`cargo run --bin auth-example`](./src/auth_example.rs) | Demonstrates Supabase authentication and authenticated API requests | - User authentication<br>- Making authenticated API calls<br>- Getting user information |
| Broadcast Example | [`cargo run --bin broadcast-example`](./src/broadcast_example.rs) | Shows how to use Supabase's broadcast feature for real-time messaging | - Real-time notifications<br>- Chat features<br>- Broadcasting messages to multiple clients |
| Presence Example | [`cargo run --bin presence-example`](./src/presence_example.rs) | Demonstrates presence tracking in channels | - Online status indicators<br>- Typing indicators<br>- User presence tracking |
| JWT Stream Example | [`cargo run --bin jwt-stream-example`](./src/jwt_stream_example.rs) | Shows JWT token management and refresh handling | - Session management<br>- Token refresh automation<br>- Maintaining authenticated sessions |
| Database Updates Example | [`cargo run --bin db-updates-example`](./src/db_updates_example.rs) | Demonstrates real-time database change listening | - Real-time data synchronization<br>- Live updates<br>- Database change notifications |
| Client Example | [`cargo run --bin client-example`](./src/client_example.rs) | Shows basic database operations with Supabase client | - Database queries<br>- CRUD operations<br>- Authenticated database requests |

## Generated schema example

The [schema bindings example](../crates/supabase-codegen-example) builds from a committed snapshot
without a database or Supabase CLI:

```sh
cargo run -p rp-supabase-codegen-example --offline
```

Dependencies must already be cached for Cargo's offline mode. The generator runs in host-side
`build.rs`; it needs no separate generator CLI. Generated rows implement `Projection<Row>`
and finite column/FK lookups. Regenerate Rust output when you upgrade `rp-supabase-codegen`; it must match the `rp-supabase-client` 0.11 runtime. JSON/JSONB markers retain `JsonColumn`.

Its `smoke.sql` and permanent `src/gaps.rs` scenario exercise shared articles/videos DTOs, typed
order, IN, JSON text paths, pagination, counts, minimal writes, raw DTO decoding, and pure
`schema::params` pairs. The runner also checks relationship projections and message CRUD/view/RPC.
Typed RPC fetches infer their return type. HTTP 204 unit-valued void behavior belongs to consumer
response tests; the live runner's RPC is `echo_message`.

For live execution, apply `smoke.sql` only to a disposable database, expose `public` through
PostgREST, and set `SUPABASE_CODEGEN_DATABASE_URL` and `SUPABASE_CODEGEN_API_URL`.
`SUPABASE_CODEGEN_API_KEY` and `SUPABASE_CODEGEN_ACCESS_TOKEN` are optional API credentials.
Remote introspection verifies TLS; use `sslmode=disable` only for a trusted local database.
See the example README for cleanup and Cargo change-detection details.

The fixtures grant access to `PUBLIC` and do not test production RLS policies. Cleanup attempts
to remove run-specific rows after recoverable errors; panics or the 30-second timeout can leave
rows behind. Live acceptance ran with Rust 1.85, PostgreSQL 17, and PostgREST 16.2.
Offline and HTTP-fixture checks are not live-server verification.

REST operations return the canonical owned error, with structured PostgREST body accessors and
response metadata. Exact numerics remain arbitrary-precision `serde_json::Number`, not `f64`.
Configured auth/REST constructors reuse a supplied HTTP client for login, refresh, and emitted
clients, with request-local API-key and bearer headers and no implicit compression override.
