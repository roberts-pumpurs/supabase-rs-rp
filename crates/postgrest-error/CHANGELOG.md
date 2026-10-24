# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.4](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-postgrest-error-v0.8.3...rp-postgrest-error-v0.8.4) - 2026-10-24

### Other

- raise the minimum Rust version to 1.88

## [0.8.3](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-postgrest-error-v0.8.2...rp-postgrest-error-v0.8.3) - 2026-10-08

### Fixed

- *(postgrest-error)* keep an independent version so rp-postgrest stays semver-compatible

### Other

- fix crate metadata, docs.rs links, and stale version notes

## [0.8.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-postgrest-error-v0.8.1...rp-postgrest-error-v0.8.2) - 2026-10-06

### Added

- Add `ErrorResponse::sqlstate()` for canonical SQLSTATE codes, excluding PostgREST and custom HTTP status codes.

## [0.8.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-postgrest-error-v0.8.0...rp-postgrest-error-v0.8.1) - 2026-10-06

### Changed

- Align the package version with the workspace patch release.

## [0.8.0] - 2026-10-06

### Changed

- Change `ErrorResponse::details` from `Option<String>` to `Option<ErrorDetails>`, with `Text` and `AmbiguousEmbeddings` variants.
- Align the crate version with the workspace release.

### Added

- Add `EmbeddingDetail` and `EmbeddingCardinality` for PostgREST's ambiguous relationship candidate arrays, including the `PGRST201` response observed on PostgREST 16.2.

## [0.7.0] - 2026-10-06

### Changed

- Version aligned with the workspace release. No API changes.

## [0.6.0] - 2026-10-06

### Changed

- Version aligned with the workspace release. No API changes.

## [0.5.0] - 2026-10-06

### Other

- Release with the workspace. No API changes.

## [0.4.0] - 2026-08-10

### Changed

- **Breaking:** replace `PostgrestUtilError` and its duplicated PostgreSQL,
  PostgREST, and custom wrapper structs with `PostgrestError`, `ErrorResponse`,
  and the lossless `ErrorCode`/`ErrorKind` model.
- Require an authoritative `http::StatusCode` when constructing a
  `PostgrestError`.
- Make `code` and `message` required when decoding a structured PostgREST
  response; malformed bodies now produce `DecodeError` instead of silently
  defaulting required fields.
- Replace unconditional status reconstruction with explicit optional
  `inferred_status` fallback behavior.

### Added

- Add `PostgrestError::from_slice` and `PostgrestError::from_vec` for decoding
  status-plus-body response data without coupling to a particular HTTP client.
- Preserve status and raw body bytes in `DecodeError`.
- Add typed classifications for current PostgREST codes through `PGRST128`,
  `PGRST205`, and `PGRST303` while retaining unknown codes losslessly.
- Add message-sensitive PostgreSQL status inference for `21000`, `22023`, and
  `42883`, plus the `57P01` administrative-shutdown mapping.
- Add `Authentication::Unknown` so authentication-sensitive status inference
  can return `None` instead of guessing.

### Fixed

- Classify `PTxyz` codes before generic PostgreSQL SQLSTATE codes and infer the
  encoded custom status (`PT402` now maps to HTTP 402).
- Map `PGRST001` and `PGRST002` to HTTP 503 and `PGRST107` to the HTTP 406
  status emitted by PostgREST.
- Stop mapping unknown PostgREST codes to HTTP 500; unrecognized valid
  PostgreSQL SQLSTATEs now use PostgREST's documented HTTP 400 fallback.
- Preserve exact SQLSTATE values instead of displaying only class patterns such
  as `08*`.
- Require the exact `PGRSTgxx` shape for PostgREST-code classification and
  treat retired `PGRST109`, `PGRST110`, and `PGRST119` codes as unknown.

## [0.3.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-postgrest-error-v0.2.3...rp-postgrest-error-v0.3.0) - 2025-05-18

### Added

- add presence ([#19](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/19))

## [0.1.1](https://github.com/roberts-pumpurs/supabase-auth-rs/compare/rp-postgrest-error-v0.1.0...rp-postgrest-error-v0.1.1) - 2024-10-26

### Other

- add changelog files
