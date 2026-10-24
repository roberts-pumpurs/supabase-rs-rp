# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-functions-v0.1.1...rp-supabase-functions-v0.1.2) - 2026-10-24

### Added

- expand Supabase clients and typed query diagnostics

### Added

- Bounded HTTP 429 retry configuration. Default POST invocations are not replayed.
- Optional native TLS and shared default transport construction.


## [0.1.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-functions-v0.1.0...rp-supabase-functions-v0.1.1) - 2026-10-24

### Other

- raise the minimum Rust version to 1.88

## [0.1.0](https://github.com/roberts-pumpurs/supabase-rs-rp/releases/tag/rp-supabase-functions-v0.1.0) - 2026-10-08

### Added

- *(functions)* add rp-supabase-functions Edge Functions client

### Fixed

- *(functions)* same-origin redirects only, redact credential headers, re-export url
- *(functions)* skip bearer for new-format keys and reject normalized names

### Other

- add crates.io and docs.rs badges; clarify realtime join replies
