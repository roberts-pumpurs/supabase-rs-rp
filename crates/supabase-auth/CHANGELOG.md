# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-auth-v0.10.0...rp-supabase-auth-v0.10.1) - 2026-10-23

### Other

- update Cargo.toml dependencies

## [0.10.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-auth-v0.9.1...rp-supabase-auth-v0.10.0) - 2026-10-09

### Added

- *(auth)* [**breaking**] group PKCE challenge fields and hide grant tokens in Debug
- *(auth)* [**breaking**] typed token grants with PKCE code exchange

## [0.9.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-auth-v0.9.0...rp-supabase-auth-v0.9.1) - 2026-10-09

### Fixed

- *(auth)* omit unset TokenRequestBody fields instead of sending null
- *(auth)* stop logging bodies, tokens, and URL queries, and never log at error level

### Other

- *(auth)* use a path-only mock dev-dependency so publishing auth does not wait on the mock crate

## [0.9.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-auth-v0.8.2...rp-supabase-auth-v0.9.0) - 2026-10-08

### Added

- *(auth)* [**breaking**] flatten auth results and add session convenience methods

### Fixed

- *(auth,postgrest,client)* follow only same-origin redirects on default clients
- *(auth)* decode both GoTrue error shapes and model confirmation-required sign-up

### Other

- add crates.io and docs.rs badges; clarify realtime join replies
- *(auth)* fix broken JwtStream doc link

## [0.8.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-auth-v0.8.1...rp-supabase-auth-v0.8.2) - 2026-10-06

### Changed

- Align the package version with the workspace release. Use PostgREST 3.2. No auth API changes.

## [0.8.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-auth-v0.8.0...rp-supabase-auth-v0.8.1) - 2026-10-06

### Fixed

- Document auth response decoding errors.

## [0.8.0] - 2026-10-06

### Added

- Configured-client constructors for auth API clients and authenticated streams.
- `JwtStream::sign_in_with_client` for login and token refresh through a caller-supplied HTTP transport.

### Changed

- Reuse the configured transport for login, refresh, and emitted auth clients.
- Apply API-key, JSON, and bearer headers per request without changing shared transport defaults or implicitly overriding compression policy.

## [0.7.0] - 2026-10-06

### Changed

- Version aligned with the workspace release. No API changes.

## [0.6.0] - 2026-10-06

### Changed

- **Breaking:** depend on `reqwest` 0.13; public error types wrap `reqwest` 0.13 errors.

## [0.5.0] - 2026-10-06

### Other

- Release with the workspace. No API changes.

## [0.3.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-auth-v0.2.3...rp-supabase-auth-v0.3.0) - 2025-05-18

### Added

- add presence ([#19](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/19))

## [0.2.3](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-auth-v0.2.2...rp-supabase-auth-v0.2.3) - 2025-03-23

### Other

- update Cargo.toml dependencies

## [0.2.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-auth-v0.2.1...rp-supabase-auth-v0.2.2) - 2025-03-16

### Added

- use stable rust

## [0.1.1](https://github.com/roberts-pumpurs/supabase-auth-rs/compare/rp-supabase-auth-v0.1.0...rp-supabase-auth-v0.1.1) - 2024-10-26

### Other

- add changelog files
