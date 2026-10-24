# Changelog

## [Unreleased]

### Added

- Shared HTTP client construction with same-origin redirects and rustls or native TLS.
- Bounded retries for explicit HTTP 429 responses. Default replay covers only GET, HEAD, and OPTIONS.
- Retry-After seconds and HTTP-date support without shortening server delays.
- Explicit mutation replay opt-in. Uncloneable streaming bodies, transport failures, and other HTTP statuses are not replayed.
