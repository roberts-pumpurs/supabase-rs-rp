# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.12.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-realtime-v0.11.0...rp-supabase-realtime-v0.12.0) - 2026-10-24

### Added

- expand Supabase clients and typed query diagnostics

### Added

- Reconnecting bounded protocol driver with per-topic join replay, fresh references,
  current auth tokens, heartbeat acknowledgement deadlines, presence retracking and
  observable connection status. Dropping the stream or explicit shutdown cancels all work.
- Scheme-driven TCP/TLS with validated IP certificates and optional native TLS backend.
- Local websocket fault-injection tests and opt-in Supabase restart acceptance scenario.
- Connect existing OAuth, anonymous, or MFA sessions without password login, or supply a configured refresh stream and session store.

### Changed

- `connect` returns `RealtimeStream`; raw input streams must be `Send + 'static`.
- Command queues are bounded and apply backpressure. Rejected joins yield `ChannelError`.
- `JoinConfig` includes `private`; add `private: false` to existing struct literals.
- Rustls remains default and takes precedence when both TLS backends are enabled.
- The obsolete fastwebsockets driver and its transport-specific `WebsocketError` and
  `HyperError` variants are replaced by `Transport(tungstenite::Error)`.
- Bound complete initial/reconnect handshakes and websocket writes with `connect_timeout`
  and `write_timeout` options and typed `ConnectTimeout` / `WriteTimeout` errors.
- Forward current-generation push replies, retain retryable authentication failures,
  retry transient server join rejections, and wait for PostgreSQL subscription recovery.
- Presence joins explicitly enable tracking. Raw `PresenceConfig` literals now include `enabled`.
- Plain WebSocket connections skip certificate loading; test peer setup cancels when client setup fails.
- Respect an installed rustls crypto provider or select AWS-LC explicitly when providers coexist.

## [0.11.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-realtime-v0.10.1...rp-supabase-realtime-v0.11.0) - 2026-10-24

### Breaking

- The re-exported `simd_json` is 0.18. Payloads, `OwnedValue` and errors use 0.18 types, and `rp_supabase_auth` is 0.11. Upgrade a direct simd-json dependency to 0.18 to match.
- Requires Rust 1.88.

## [0.10.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-realtime-v0.10.0...rp-supabase-realtime-v0.10.1) - 2026-10-23

### Other

- update Cargo.toml dependencies

## [0.10.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-realtime-v0.9.1...rp-supabase-realtime-v0.10.0) - 2026-10-09

### Other

- *(realtime,mock)* pin the upcoming 0.10 release and use typed login credentials

## [0.9.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-realtime-v0.9.0...rp-supabase-realtime-v0.9.1) - 2026-10-09

### Other

- *(realtime)* remove duplicated 0.1.1 changelog sections that break release notes

## [0.9.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-realtime-v0.8.2...rp-supabase-realtime-v0.9.0) - 2026-10-08

### Added

- *(realtime)* [**breaking**] typed postgres changes, inferred channel constructors, fixed type names

### Fixed

- *(realtime)* decode join replies for insert, update, and delete subscriptions
- *(realtime)* surface channel errors, keep presence name, document delete RLS limits

### Other

- *(realtime)* cover typed_changes ordering, missing records, and reply events
- add crates.io and docs.rs badges; clarify realtime join replies

## [0.8.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.8.1...rp-supabase-realtime-v0.8.2) - 2026-10-06

### Changed

- Align the package version with the workspace release. No realtime API changes.

## [0.8.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.8.0...rp-supabase-realtime-v0.8.1) - 2026-10-06

### Fixed

- Require patched `bytes` and `rustls` versions.
- Document connection errors and extract the existing heartbeat stream into a private helper.

## [0.8.0] - 2026-10-06

### Changed

- Version aligned with the workspace release. No API changes.

## [0.7.0] - 2026-10-06

### Changed

- Version aligned with the workspace release. No API changes.

## [0.6.0] - 2026-10-06

### Changed

- Version aligned with the workspace release. No API changes.

## [0.5.0] - 2026-10-06

### Other

- Release with the workspace. No API changes.

## [0.3.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.2.3...rp-supabase-realtime-v0.3.0) - 2025-05-18

### Added

- add presence ([#19](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/19))

## [0.2.3](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.2.2...rp-supabase-realtime-v0.2.3) - 2025-03-23

### Fixed

- realtime will handle fatal errors

## [0.2.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.2.0...rp-supabase-realtime-v0.2.1) - 2025-01-04

### Added

- add extra null case handlings for realtime messages

## [0.2.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.1.2...rp-supabase-realtime-v0.2.0) - 2024-11-24

### Added

- presence typings
- add broadcast

## [0.1.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-realtime-v0.1.1...rp-supabase-realtime-v0.1.2) - 2024-10-26

### Other

- release v0.1.1 ([#12](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/12))

## [0.1.1](https://github.com/roberts-pumpurs/supabase-rs-utils/releases/tag/rp-supabase-realtime-v0.1.1) - 2024-10-26

### Added

- created an auth api module
- created simple supabase client wrapper
- can deserialize postgres changes packets
- realtime can receive proper updates
- add phx_close event
- supabase realtime will decode the incoming json
- add hb capabilities
- update deps
- query builder keep hold of table name
- base for supabase client
- steriliaztion tests ([#3](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/3))
- configurable params for stream
- added error phx_reply
- updated the realtime connectoin example
- async ws handler will call the waker of LiveRealtimeConnection
- fir realtime connection implementaiton
- improved url resolvement in supabase-realtime
- example `fastwebsockets` implementation

### Fixed

- broken example

### Other

- release
- init changelog
- update cargo.toml
- add changelog files
- readme update
- ai generate readmes
- internal crate renaming
- fmt
- remove unused deps
- examples split into own crate
- created client / server architecture for realtime data
- simplify jwt refresh
- replace serde_json with simd_json
- docs & renaming
- send phx join message
- fix broken tests
- split ws connection handling into modules
