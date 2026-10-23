# Changelog

## [Unreleased]

## [0.13.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-codegen-v0.13.0...rp-supabase-codegen-v0.13.1) - 2026-10-23

### Other

- syn moves to 3, prettyplease to 0.3, and sqlparser to 0.63. Released with rp-supabase-client 0.13.1, which reports unknown `select!` names in words. ([#39](https://github.com/roberts-pumpurs/supabase-rs-rp/pull/39))

## [0.13.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-codegen-v0.12.0...rp-supabase-codegen-v0.13.0) - 2026-10-20

### Breaking

- Snapshot format is version 4. Regenerate version 3 snapshots. Public `model::Table` metadata now requires `computed_fields`.
- Insert and update fields for `NOT NULL` JSON/JSONB columns, and strict non-null JSON RPC arguments, use `schema::NonNullJson` instead of `serde_json::Value`. Writing JSON null to those columns no longer compiles. PostgREST turned it into SQL `NULL`, which the column rejected at runtime.

### Added

- Generate PostgREST computed fields. A same-schema function whose only input is a table or view row type gets a nullable column marker. It works with `select!`, `projection!`, filters, ordering, and embedded filters. `Row`, `Insert`, and `Update` do not include it. `snapshot check` reports computed-field drift by name. ([#37](https://github.com/roberts-pumpurs/supabase-rs-rp/pull/37))

### Other

- Example fixtures and docs use neutral table and function names.

## [0.12.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-codegen-v0.11.0...rp-supabase-codegen-v0.12.0) - 2026-10-09

### Other

- *(client)* [**breaking**] re-exported rp-supabase-auth moves to 0.10 with typed credentials

## [0.11.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-codegen-v0.10.0...rp-supabase-codegen-v0.11.0) - 2026-10-08

### Other

- point version pins at the upcoming release and use badges in the crate table
- add crates.io and docs.rs badges; clarify realtime join replies
- *(client,codegen)* compile README snippets through the example schema
- fix crate metadata, docs.rs links, and stale version notes

## [0.10.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-codegen-v0.9.3...rp-supabase-codegen-v0.10.0) - 2026-10-06

### Breaking

- Snapshot format is version 3. Regenerate version 2 snapshots. Public `model::Argument` metadata now requires `nullable`.

### Added

- Honor nullability contracts in snapshots, SQL comments, and `Generator::not_null`.
- Infer non-null columns for safe plain-column projections in ordinary single-table views.
- Generate Serde and filtering enums for exact, validated string CHECK membership constraints.
- Add `.column_type(...)` overrides for application-owned Rust types.
- Add `.json_type(...)` for columns, composites, RPC arguments, OUT records, and returns. Preserve SQL wrappers.
- Add `.reexport_macros()` to export schema-local `select!` and `key!` with the runtime path built in.
- Add unambiguous forward FK-column and reverse table aliases. Support explicit `.relationship_alias(...)` mappings.
- Add `.strict_args()` and `.strict_args_for(...)`. SQL `@nullable` annotations preserve nullable inputs. Default arguments still support omission.
- Add the optional `cli` binary. `snapshot write` exports metadata. `snapshot check` reports drift without writing files.

### Fixed

- Keep materialized-view fields nullable unless a contract explicitly declares them non-null.
- Keep CHECK fields as strings when their SQL equality rules do not match Rust string equality.
- Validate customization targets against emitted bindings and resolve overrides through generated SQL identifiers.
- Exclude overridden SQL fields from dependency collection.

## [0.9.3](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-codegen-v0.9.2...rp-supabase-codegen-v0.9.3) - 2026-10-06

### Other

- *(schema)* share column contracts and projection compilation

### Changed

- Resolve relation column nullability and write obligations once for row fields, column markers,
  insert/update payloads, and automatic `Default` eligibility. Generated contracts and snapshot
  version 2 remain unchanged.

## [0.9.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-codegen-v0.9.1...rp-supabase-codegen-v0.9.2) - 2026-10-06

### Changed

- Align the version with client 0.9.2. Generated bindings are unchanged.

## [0.9.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-codegen-v0.9.0...rp-supabase-codegen-v0.9.1) - 2026-10-06

### Fixed

- Require PostgreSQL client 0.19.14 with patched protocol decoding and bounded SCRAM authentication.
- Link the package API documentation.
- Publish committed source through release-plz. Version 0.9.0 archives retain their original dirty-tree provenance.

## [0.9.0] - 2026-10-06

### Added

- Direct finite column and FK lookup implementations for query-first selections on exact schema-qualified rows.
- Lossless character-type names from the actual emitted Rust identifiers, preserving original SQL keys and FK hints.

### Changed

- Regenerate Rust output for client 0.9. Snapshot version 2 and existing relationship capability limits remain unchanged.

## [0.8.0] - 2026-10-06

### Changed

- **Breaking:** generated rows implement relation-parameterized `Projection<Row>`. Regenerate Rust bindings with codegen 0.8 for runtime 0.8; snapshots remain version 2.
- Emit `JsonColumn` markers for JSON and JSONB columns, including domains over those types.
- Update usage for shared projection DTOs, pure typed query pairs, typed pagination and counts, minimal writes, and raw checked DTO decoding.
- Document inferred typed RPC returns, HTTP 204 unit responses, canonical REST errors, and exact numeric decoding.

## [0.7.0] - 2026-10-06

### Added

- Generated column markers, typed relation query entry points, row projections, nullable
  predicates, and base-table write contracts.
- Generated enum display implementations that preserve database labels for typed filters.
- Forward and reverse FK markers with exact constraint hints and PK/UNIQUE-derived reverse cardinality.
- Ordered FK, key, qualified target, and partition metadata acquired from PostgreSQL catalogs.

### Changed

- **Breaking:** snapshots use version 2. Regenerate version 1 snapshots from PostgreSQL.
- Keep dependency-schema types separate from explicitly selected table and RPC endpoints.

## [0.6.0] - 2026-10-06

### Changed

- Document schema-only runtime use through `rp-supabase-client` with default features disabled.

## [0.5.0] - 2026-10-06

### Added

- Build-script-first Rust schema generation from versioned snapshots or direct PostgreSQL catalog introspection.
- Row, insert, update, enum, composite, domain, array, and named-argument RPC contracts.
- Prelude imports, custom derives, global and per-type macro attributes, SQL type overrides, and runtime path configuration.
- Optional verified-TLS database acquisition, Cargo input tracking, snapshot export, and deterministic `OUT_DIR` output.
- An executable offline example and a finite live PostgREST verification scenario.
