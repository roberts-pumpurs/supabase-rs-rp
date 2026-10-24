# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Typed public, authenticated streaming, and signed image transformations.
- Bounded-memory TUS 1.0 upload sessions with persisted URL handles, HEAD resume,
  offset reconciliation, cancellation-safe dirty state, and strict chunk sizing.
- Shared configurable 429 retries for storage requests; resumable PATCH never replays.

### Fixed

- Preserve TUS overwrite intent on every HEAD/PATCH; reopening explicitly accepts `upsert`.
- Reject remote signed-image JWT path claims that do not exactly match the requested
  bucket/key, reporting an upstream signing mismatch without exposing the token.
- Do not PATCH a confirmed complete TUS upload. Creation already completes an empty object.

## [0.2.2](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-storage-v0.2.1...rp-supabase-storage-v0.2.2) - 2026-10-24

### Other

- raise the minimum Rust version to 1.88

## [0.2.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-storage-v0.2.0...rp-supabase-storage-v0.2.1) - 2026-10-23

### Other

- update Cargo.toml dependencies

## [0.2.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-storage-v0.1.2...rp-supabase-storage-v0.2.0) - 2026-10-09

### Added

- *(storage)* [**breaking**] decode the error body code as StorageErrorCode and make message optional

## [0.1.2](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-storage-v0.1.1...rp-supabase-storage-v0.1.2) - 2026-10-09

### Added

- *(storage)* add streaming downloads, single-object delete, and StorageError::api_status

## [0.1.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-storage-v0.1.0...rp-supabase-storage-v0.1.1) - 2026-10-09

### Added

- *(storage)* add StorageClient::with_project_url to change the base URL and keep headers

## [0.1.0](https://github.com/roberts-pumpurs/supabase-rs-rp/releases/tag/rp-supabase-storage-v0.1.0) - 2026-10-08

### Added

- *(storage)* add rp-supabase-storage Storage API client

### Fixed

- *(storage)* follow only same-origin redirects on the default client
- *(storage,supabase)* sign single objects via batch endpoint; default bearer for legacy keys
- *(storage)* correct signed URLs, bucket option nulls, cache default, and key headers

### Other

- add crates.io and docs.rs badges; clarify realtime join replies
