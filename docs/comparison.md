# Comparison with other Rust Supabase clients

Checked in 2026-10. Projects change often; read their current docs before you decide.

This page compares this workspace (`supabase-rp` and its crates) with three other crates:

- [supabase_rs](https://github.com/xylex-group/supabase_rs) 0.8.0 ([README](https://raw.githubusercontent.com/xylex-group/supabase_rs/main/README.md), [Cargo.toml](https://raw.githubusercontent.com/xylex-group/supabase_rs/main/Cargo.toml), [crates.io](https://crates.io/crates/supabase_rs)).
- [postgrest](https://github.com/supabase-community/postgrest-rs) 1.6.0, last released 2023-07 ([README](https://raw.githubusercontent.com/supabase-community/postgrest-rs/master/README.md), [crates.io](https://crates.io/crates/postgrest)).
- [rust_supabase_sdk](https://github.com/Lenard-0/Rust-Supabase-SDK) 0.4.4 ([README](https://raw.githubusercontent.com/Lenard-0/Rust-Supabase-SDK/main/README.md), [crates.io](https://crates.io/crates/rust_supabase_sdk)).

"Unknown" means the primary sources above do not say.

| Topic | supabase-rp (this workspace) | supabase_rs | postgrest | rust_supabase_sdk |
|-------|------------------------------|-------------|-----------|-------------------|
| Typed results | `fetch::<T>()` decodes into your serde types | No. Rows are `serde_json::Value`. | No. Returns the raw HTTP response; you decode the body. | Yes. `Vec<T>` from the string builder and typed builder. |
| Compile-time column checks | Yes, with generated types (`select!`, typed columns) | No. Columns and values are strings. | No | Yes, with generated `Column<R, V>` constants |
| Codegen | `build.rs` generator from a snapshot or a live database | `typegen` feature | No | `cargo supabase gen types` CLI |
| Auth: user sessions | Password, OTP, OAuth PKCE, anonymous sign-in, sign up, user update, sign out | Not listed | No | Email, phone, OTP, OAuth, anonymous; pluggable session stores |
| Auth: refresh | Refresh stream with pluggable memory/file stores and refresh-only resume | Not listed | No | Unknown |
| Auth: MFA and admin | Admin user/factor management, invitations, generated links, TOTP and phone MFA, informational AAL | Not listed | No | Admin user management. MFA unknown. |
| Realtime | Typed database changes, broadcast, presence; bounded reconnect, join replay, presence restore, status stream | Not listed | No | `postgres_changes`, broadcast, presence (opt-in feature), with automatic reconnect |
| Storage | Buckets, objects, signed/public URLs, image transforms, resumable TUS uploads with resume and upsert | Downloads only (`storage` feature) | No | Buckets, objects, signed URLs, image transforms |
| Edge functions | Invoke with JSON or raw bodies; `send()` returns the response for streaming | Not listed | No | Invoke, with streamed responses |
| GraphQL | Typed responses with variables, operation names, execution errors and partial data | Yes, experimental (`graphql` feature) | No | Not listed |
| Error model | `thiserror` enums; PostgREST errors parsed into typed codes (`rp-postgrest-error`) | `Error` enum with HTTP status and provider details | Unknown | Unknown |
| TLS options | Default rustls and optional native TLS for HTTP and WebSockets. Supplied HTTP clients retain their policy. Live codegen uses native TLS. | `rustls` or `native_tls` feature | Unknown for 1.6.0 | `rustls` (default) or `native-tls` |
| Retries | Bounded 429 retries with `Retry-After`; read methods by default, mutation replay requires explicit opt-in | Unknown | No | Retries on HTTP 429 in the shared HTTP transport ([source](https://github.com/Lenard-0/Rust-Supabase-SDK/blob/5b6185e416ac0f1da2180f1b3f49f4fca9cb71ad/src/universals/mod.rs#L270-L289)) |
| MSRV | 1.88 | 1.85 | Not declared | Declares 1.75; its required `uuid` 1.23.1 needs 1.85 |

## Remaining tradeoffs

- `supabase_rs` and this workspace both support string-based CRUD. Here it uses `client.from("table").select("columns").fetch::<T>()`.
- `postgrest` has one job. Use this workspace's standalone `rp-postgrest` crate when you do not need the other services.
- This workspace requires Rust 1.88. The lower declared versions in this table are not all effective dependency minimums.
- OAuth needs application-controlled browser redirects. File session stores contain plaintext credentials and require a private directory.
- Reconnect does not restore messages sent during an outage. Mutation retries do not provide exactly-once execution.
- Current Supabase Storage single-object signing can encode a `?` in the signed claim incorrectly. Transformed signing rejects a mismatched claim instead of returning an unusable URL. See the [upstream signer](https://github.com/supabase/storage/blob/master/src/http/routes/object/getSignedURL.ts).

## Where this workspace differs

- Generated types come from `build.rs`, not a separate CLI. A committed snapshot lets builds run offline.
- PostgREST errors decode into typed PostgreSQL and PostgREST error codes.
- Each service is a separate crate. You can depend on one service without the others.
