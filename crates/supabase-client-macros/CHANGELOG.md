# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-macros-v0.13.0...rp-supabase-client-macros-v0.13.1) - 2026-10-23

### Fixed

- `select!` resolves each column and relationship through a unit struct named after the written identifier, located at that identifier. An unknown name is one error at the name, in words. ([#39](https://github.com/roberts-pumpurs/supabase-rs-rp/pull/39))
- `projection!` shared filter key methods name the filter when used with a relation outside the shared list.

### Other

- syn moves to 3.

## [0.11.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-macros-v0.10.0...rp-supabase-client-macros-v0.11.0) - 2026-10-08

### Other

- fix crate metadata, docs.rs links, and stale version notes

## [0.10.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-macros-v0.9.3...rp-supabase-client-macros-v0.10.0) - 2026-10-06

### Added

- Shared `projection!` declarations accept `filters { key: [column_for_first_relation, column_for_second_relation] }` with compile-time value/filter type equality and exact relation ownership.

## [0.9.3](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-macros-v0.9.2...rp-supabase-client-macros-v0.9.3) - 2026-10-06

### Other

- *(schema)* share column contracts and projection compilation

### Changed

- Compile named projections through hidden support behind the runtime's `$crate`-preserving adapter.
  Named and local records share one strict decoder emitter and one selected-field renderer.

## [0.9.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-macros-v0.9.1...rp-supabase-client-macros-v0.9.2) - 2026-10-06

### Changed

- Align the version with client 0.9.2. Macro behavior is unchanged.

## [0.9.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-macros-v0.9.0...rp-supabase-client-macros-v0.9.1) - 2026-10-06

### Fixed

- Link the package API documentation.
- Publish committed source through release-plz. Version 0.9.0 archives retain their original dirty-tree provenance.

## [0.9.0] - 2026-10-06

### Added

- Query-first `select!` expressions and lossless `key!` names for client 0.9.
