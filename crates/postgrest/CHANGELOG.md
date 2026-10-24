# Changelog

## [Unreleased]

## [3.2.2](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-postgrest-v3.2.1...rp-postgrest-v3.2.2) - 2026-10-24

### Other

- raise the minimum Rust version to 1.88

## [3.2.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-postgrest-v3.2.0...rp-postgrest-v3.2.1) - 2026-10-08

### Fixed

- *(auth,postgrest,client)* follow only same-origin redirects on default clients

### Other

- fix crate metadata, docs.rs links, and stale version notes

## [3.2.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-postgrest-v3.1.0...rp-postgrest-v3.2.0) - 2026-10-06

### Added

- Add `Error::server_response()` to retain non-success status even without a decoded error body, and `Error::is_jwt_expired()` for explicit JWT expiration messages.
- Add the `test-util` feature with `Error::from_response(status, body)` using the production decoder. Successful statuses return `None`.

## [3.1.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-postgrest-v3.0.0...rp-postgrest-v3.1.0) - 2026-10-06

### Added

- `Error::postgrest_response()` returns observed HTTP status and the borrowed typed server error body together.

## [3.0.0] - 2026-10-06

### Added

- Workspace-owned concrete PostgREST implementation with the `rp_postgrest` library name and Rust 1.85 minimum.
- Fallible default/configured client constructors. Supplied Reqwest transports retain connection pools, headers, timeout, proxy, and TLS policy.
- Checked execution that rejects all non-2xx statuses, including HTTP 300, plus public `fetch<T>` with centralized successful JSON decoding.
- One flat canonical error with structured-body getters and observed status, headers, and effective URL. Malformed error envelopes retain exact bytes. Successful body-read and decoding errors retain response metadata.
- JSON serialization helpers with deferred failures; safe `in_values` for literal list elements alongside raw `in_` grammar.
- Exact/planned/estimated count modes, data-plus-total fetch, HEAD read counts, minimal-return mutations, and affected-row counts. Missing or invalid totals report typed errors.
- HTTP 204 unit decoding for void responses. Empty HTTP 200 bodies remain JSON errors.

### Changed

- Breaking: replace the old `postgrest` library name with `rp_postgrest`; remove obsolete execution/query error aliases and public mutable request state.
- Breaking: client construction, client auth/header changes, and request build return errors instead of panicking or assuming valid configuration. `Builder` is not Clone.
- Breaking: resource names are literal SQL names encoded exactly once. Dot-only resources fail explicitly rather than addressing another endpoint.
- Breaking: count preferences no longer force Range 0-0. Caller pagination remains intact; use `execute_count` for body-free totals.
- Compose Prefer directives by key with last-value wins and preserve unrelated directives. Repeated order calls compose one effective order value while repeated filters retain their order.
- Enable rustls by default; no-default builds enable no TLS backend through this crate. HTTPS requires a TLS backend in the dependency graph.

The implementation adapts MIT/Apache-2.0 code from rp-postgrest 2.1.0. See the README for migration and raw grammar contracts.
