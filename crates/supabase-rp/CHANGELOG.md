# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/supabase-rp-v0.5.0...supabase-rp-v0.6.0) - 2026-10-24

### Breaking

- The re-exported auth, realtime and typed client crates move to rp-supabase-auth 0.11, rp-supabase-realtime 0.11 and rp-supabase-client 0.14. They use simd-json 0.18.
- Requires Rust 1.88.

## [0.5.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/supabase-rp-v0.4.0...supabase-rp-v0.5.0) - 2026-10-20

### Breaking

- `supabase_rp::typed` is rp-supabase-client 0.13. Generated writes to `NOT NULL` JSON columns take `NonNullJson`, and snapshots use format 4. See the [rp-supabase-client migration notes](../supabase-client/README.md#migration-from-012).

## [0.4.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/supabase-rp-v0.3.0...supabase-rp-v0.4.0) - 2026-10-09

### Other

- *(supabase-rp)* [**breaking**] re-exported storage moves to 0.2 with typed error codes

## [0.3.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/supabase-rp-v0.2.0...supabase-rp-v0.3.0) - 2026-10-09

### Other

- *(supabase-rp)* [**breaking**] re-exported auth and realtime move to 0.10

## [0.2.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/supabase-rp-v0.1.0...supabase-rp-v0.2.0) - 2026-10-09

### Added

- *(supabase-rp)* [**breaking**] typed enables only the schema runtime, and add typed-client, test-util, and with_storage_url

### Other

- *(supabase-rp)* pin the upcoming 0.2 release

## [0.1.0](https://github.com/roberts-pumpurs/supabase-rs-rp/releases/tag/supabase-rp-v0.1.0) - 2026-10-08

### Added

- *(supabase-rp)* [**breaking**] rename umbrella crate to supabase-rp with short module re-exports
