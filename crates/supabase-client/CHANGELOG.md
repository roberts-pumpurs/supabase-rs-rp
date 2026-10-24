# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.15.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-v0.14.0...rp-supabase-client-v0.15.0) - 2026-10-24

### Added

- expand Supabase clients and typed query diagnostics

### Breaking

- Named value keys carry `Key<K, Name>`; local explicit annotations use `Key<key!(type id), _>`.
- Handwritten relations must provide `ColumnLookup`; regenerate bindings with the matching codegen release.

### Added

- Optional native TLS and the shared bounded HTTP 429 retry policy.

### Fixed

- Concrete selection errors name the SQL table and list real columns. Generic roots keep readable fallback diagnostics.
- Key lookup errors name the written identifier, including root, relationship-handle, and composed-path lookups.
- Private record markers no longer appear in missing-field suggestions. Raw/Unicode fields remain movable and retain their SQL keys.
- Runtime aliases can have the same name as selected columns, relationships, or keys.


## [0.14.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-v0.13.1...rp-supabase-client-v0.14.0) - 2026-10-24

### Breaking

- The re-exported `rp_supabase_auth` moves to 0.11, which uses simd-json 0.18.
- Requires Rust 1.88.

## [0.13.1](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-v0.13.0...rp-supabase-client-v0.13.1) - 2026-10-23

### Fixed

- An unknown column or relationship in `select!` is one compile error at that name: ``no column `title` on `Row` ``. Before, rustc printed the name as a tuple of `Character` types, listed every valid key the same way, and reported it twice. ([#39](https://github.com/roberts-pumpurs/supabase-rs-rp/pull/39))
- A `projection!` shared filter used with a relation outside its list names the filter: ``shared filter `content_id` of `Content` has no column on `Row` ``.
- `ColumnByKey` and `RelationshipByKey` failures, for example `selected.column(key!(titl))`, say "has no column for this key" instead of "the trait bound is not satisfied".

### Other

- Dependencies move to their newest releases that support Rust 1.85. This clears RUSTSEC-2026-0382, 0383, 0385 and 0386 in the lockfile.

## [0.13.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-v0.12.0...rp-supabase-client-v0.13.0) - 2026-10-20

### Breaking

- Regenerate snapshots and bindings with codegen 0.13 (snapshot format 4). Generated writes to `NOT NULL` JSON/JSONB columns take `schema::NonNullJson`. See [Migration from 0.12](README.md#migration-from-012). ([#37](https://github.com/roberts-pumpurs/supabase-rs-rp/pull/37))

### Added

- `schema::NonNullJson`, a JSON value that is never null, for generated write payloads on `NOT NULL` json/jsonb columns.

## [0.12.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-v0.11.0...rp-supabase-client-v0.12.0) - 2026-10-09

### Other

- *(client)* [**breaking**] re-exported rp-supabase-auth moves to 0.10 with typed credentials

## [0.11.0](https://github.com/roberts-pumpurs/supabase-rs-rp/compare/rp-supabase-client-v0.10.0...rp-supabase-client-v0.11.0) - 2026-10-08

### Fixed

- *(auth,postgrest,client)* follow only same-origin redirects on default clients

### Other

- *(client)* [**breaking**] re-exported rp-supabase-auth moves to 0.9 with flat results
- *(client)* cover same-origin redirect policy and document it
- add crates.io and docs.rs badges; clarify realtime join replies
- *(client,codegen)* compile README snippets through the example schema
- fix crate metadata, docs.rs links, and stale version notes

## [0.10.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.9.3...rp-supabase-client-v0.10.0) - 2026-10-06

### Added

- Shared projection filter keys map relation-specific columns through zero-sized typed markers without adding selected DTO fields.
- Runtime scalar filters, checked nested AND/OR query pairs, and composite ascending keyset cursors escape literal identifiers and values.

## [0.9.3](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.9.2...rp-supabase-client-v0.9.3) - 2026-10-06

### Other

- *(schema)* share column contracts and projection compilation

### Changed

- Named `projection!` and query-local `select!` share strict decoder emission and exact-capacity selection rendering.
  Both grammars, shared DTO contracts, and typed handle ownership stay unchanged.

## [0.9.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.9.1...rp-supabase-client-v0.9.2) - 2026-10-06

### Added

- RPC single-object mode, caller-selected JSON decoding with `fetch_as`, and checked status-only `execute`.
- Pure limit, offset, and inclusive range query pairs, with errors for reversed or overflowing ranges.
- Runtime selections, literal-column ordering, and embedded relation scopes without generated relation markers.

## [0.9.1](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.9.0...rp-supabase-client-v0.9.1) - 2026-10-06

### Fixed

- Declare Rust 1.85 support and link the package documentation.
- Publish committed source through release-plz. Version 0.9.0 archives retain their original dirty-tree provenance.

## [0.9.0] - 2026-10-06

### Added

- Query-first `select!` expressions with inferred scalar and FK child records, typed handles, and function-local/generic type support.
- Lossless `key!` names and schema-qualified column/FK lookup contracts.
- Inline inner and predicate-only empty embeds, named child reuse, and composed named DTO descendant paths.
- Strict local record decoding and optional field-dependent `Debug`/`Serialize` implementations.

### Changed

- Breaking: `Query::select` takes a selection value. Replace `.select::<Dto>()` with `.select(named::<_, Dto>())`.
- Regenerate Rust bindings with codegen 0.9. Snapshot version 2 and named `projection!` DTOs remain supported.

## [0.8.0] - 2026-10-06

### Added

- Shared projection DTOs over several generated relations with compile-time selected-field type and exact response-key checks.
- Typed multi-column ordering, explicit null placement, literal IN lists, and JSON text-path equality.
- Read pagination with a Paged state that retains read operations but cannot become a mutation.
- Data-plus-total fetches, body-free read counts, minimal-return writes, and affected-row counts.
- Pure typed query-pair rendering through `schema::params`, without constructing an HTTP client or request.
- Typed RPC fetches that infer generated return types and share the owned client's success decoder.
- Configured anonymous and authenticated constructors sharing a supplied transport for REST, login, and token refresh.

### Changed

- Breaking: use the workspace-owned rp-postgrest 3.0 and its `rp_postgrest` library/reexport. Remove `postgrest`, `PostgerstResponse`, `ResponseError`, and `schema::QueryError`; checked fetches return one canonical `Error` rather than nested response results.
- Breaking: projections implement `Projection<R>` instead of declaring an associated relation. Generated bindings must be regenerated with codegen 0.8; embed paths now retain their source relation.
- Breaking: `into_raw()` returns the owned Builder directly, without `?`. Decode raw rows with `fetch::<Vec<P>>()`; obsolete raw-result/response decode helpers are removed.
- Breaking: direct client construction, client auth/header configuration, and raw build are fallible. Builders are not Clone; recreate requests from a shared Postgrest client.
- Breaking: count preferences retain caller pagination instead of forcing Range 0-0. Paged typed reads cannot transition into writes.
- Pass literal resource names for one-time encoding; dot-only resources now fail explicitly.
- Preserve canonical error body access, HTTP 300 ambiguous-relationship details, response metadata, exact numeric decoding, relationship cardinality, RLS behavior, projection locks, and server-native DELETE limitations.

## [0.7.0] - 2026-10-06

### Added

- Typed query builders with generated column ownership, scalar filters, nullable predicates,
  inferred row decoding, checked HTTP errors, and typed insert/update/delete payloads.
- Named `projection!` results that derive field types and selections from generated schema bindings.
- Explicit `into_raw()` access for expressions outside typed queries.
- Nested FK projections with named aliases, inner joins, predicate-only empty embeds,
  typed child filter paths, and existence or anti-existence predicates.
- Projection locking after embedded predicates, including typed write queries.

### Changed

- **Breaking:** replace `schema::from::<Row>(client)` with typed `schema::query::<Row>(client)`
  or the generated relation's `query(client)`.
- **Breaking:** custom `Projection` implementations must provide `SELECT_LEN` and `write_selection`.

## [0.6.0] - 2026-10-06

### Added

- A default `client` feature. Disable default features to depend only on the generated-binding
  `schema` runtime, without `rp-supabase-auth` or `serde_json/arbitrary_precision`.

### Changed

- **Breaking:** depend on `rp-postgrest` 2.1 and `reqwest` 0.13.
- **Breaking:** re-export the PostgREST client as `postgrest` (its library name since
  `rp-postgrest` 2) instead of `rp_postgrest`.

## [0.5.0] - 2026-10-06

### Added

- Generated-schema integration with relation and RPC metadata, typed field omission,
  nullable multidimensional arrays, a runtime prelude, and `include_schema!`.

### Changed

- Decode JSON responses with arbitrary-precision Serde JSON, preserving PostgreSQL
  numeric values and avoiding a mutable response-body copy.
- **Breaking:** `ResponseError::Json` now contains `serde_json::Error` instead of
  `simd_json::Error`.

## [0.4.0] - 2026-08-10

### Changed

- **Breaking:** return `rp_postgrest_error::PostgrestError` from response JSON
  helpers.
- Preserve the authoritative HTTP response status when decoding PostgREST
  errors.
- Rename the misspelled `IntrenalError` type to `ResponseError` and simplify
  its variant names.
- Report malformed PostgREST error bodies through
  `ResponseError::PostgrestDecode` with the original status and body.

## [0.3.0](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.2.3...rp-supabase-client-v0.3.0) - 2025-05-18

### Added

- add presence ([#19](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/19))

## [0.2.3](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.2.2...rp-supabase-client-v0.2.3) - 2025-03-23

### Other

- update Cargo.toml dependencies

## [0.1.2](https://github.com/roberts-pumpurs/supabase-rs-utils/compare/rp-supabase-client-v0.1.1...rp-supabase-client-v0.1.2) - 2024-10-26

### Other

- release v0.1.1 ([#12](https://github.com/roberts-pumpurs/supabase-rs-utils/pull/12))

## [0.1.1](https://github.com/roberts-pumpurs/supabase-rs-utils/releases/tag/rp-supabase-client-v0.1.1) - 2024-10-26

### Added

- created an auth api module
- re-export postgrest error
- created simple supabase client wrapper
- supabase request builders
- cleanup the execute method
- smart postgrest error handling
- authenticated user client will expose user struct
- query builder keep hold of table name
- wip exapmle for crud ops
- execute queries
- base for supabase client
- initial client wrapper

### Fixed

- clone internal vec

### Other

- release
- init changelog
- add changelog files
- get rid of a git dep
- readme update
- ai generate readmes
- internal crate renaming
- fmt
- fmt
- postgrest errors extracted as a crate, dropped supabase-client
- fmt
- instrument the code
- export the error
- remove unwraps in auth code
- linter errors
- replace serde_json with simd_json

