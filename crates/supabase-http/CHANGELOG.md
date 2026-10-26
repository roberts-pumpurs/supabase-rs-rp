# Changelog

## [Unreleased]

## [0.1.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-http-v0.1.0...rp-supabase-http-v0.1.1) - 2026-10-26

### Fixed

- Box the retry future to keep nested caller futures small and avoid debug-build stack overflows.
- Retain only the next request across awaits instead of a request and a retry template.

## [0.1.0](https://github.com/roberts-pumpurs/supabase-rs-rp/releases/tag/rp-supabase-http-v0.1.0) - 2026-10-24

### Added

- expand Supabase clients and typed query diagnostics

### Added

- Shared HTTP client construction with same-origin redirects and rustls or native TLS.
- Bounded retries for explicit HTTP 429 responses. Default replay covers only GET, HEAD, and OPTIONS.
- Retry-After seconds and HTTP-date support without shortening server delays.
- Explicit mutation replay opt-in. Uncloneable streaming bodies, transport failures, and other HTTP statuses are not replayed.
