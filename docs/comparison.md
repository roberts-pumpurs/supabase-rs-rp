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
| Auth: user sessions | Password, OTP, sign up, user update, sign out | Not listed | No | Email, phone, OTP, OAuth, anonymous; pluggable session stores |
| Auth: refresh | `refresh_session` and a refresh stream | Not listed | No | Unknown |
| Auth: MFA and admin | Request types for MFA factors and admin endpoints; no convenience methods | Not listed | No | Admin user management. MFA unknown. |
| Realtime | Database changes (typed), broadcast, presence. No websocket reconnect. | Not listed | No | `postgres_changes`, broadcast, presence (opt-in feature), with automatic reconnect |
| Storage | Buckets, objects, signed and public URLs. No TUS, no image transforms. | Downloads only (`storage` feature) | No | Buckets, objects, signed URLs, image transforms |
| Edge functions | Invoke with JSON or raw bodies; `send()` returns the response for streaming | Not listed | No | Invoke, with streamed responses |
| GraphQL | No | Yes, experimental (`graphql` feature) | No | Not listed |
| Error model | `thiserror` enums; PostgREST errors parsed into typed codes (`rp-postgrest-error`) | `Error` enum with HTTP status and provider details | Unknown | Unknown |
| TLS options | HTTP defaults to rustls; pass your own `reqwest::Client` to choose another backend. Realtime uses rustls. Live codegen uses native TLS. | `rustls` or `native_tls` feature | Unknown for 1.6.0 | `rustls` (default) or `native-tls` |
| Retries | Not built in for HTTP requests | Unknown | No | Retries on HTTP 429 in the shared HTTP transport ([source](https://github.com/Lenard-0/Rust-Supabase-SDK/blob/5b6185e416ac0f1da2180f1b3f49f4fca9cb71ad/src/universals/mod.rs#L270-L289)) |
| MSRV | 1.88 | 1.85 | Not declared | Declares 1.75; its required `uuid` 1.23.1 needs 1.85 |

## Where others are ahead

- supabase_rs has GraphQL support (experimental) and a short string-based API for simple CRUD. It is one crate with one client.
- postgrest is small and has one job. Use it if you want a minimal PostgREST builder and decode the body yourself.
- rust_supabase_sdk covers more of the Supabase surface in one crate: OAuth and anonymous sign-in, admin user methods, storage image transforms, realtime reconnect with channel replay, and retries on HTTP 429. It also has a native TLS feature.

## Where this workspace differs

- Generated types come from `build.rs`, not a separate CLI. A committed snapshot lets builds run offline.
- PostgREST errors decode into typed PostgreSQL and PostgREST error codes.
- Each service is a separate crate. You can depend on one service without the others.
