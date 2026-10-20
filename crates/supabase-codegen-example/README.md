# Schema bindings example

`build.rs` generates bindings from `schema.json` by default. It adds a prelude type alias, a text mapping, `PartialEq`, and a targeted `TypedBuilder` derive. `src/main.rs` includes the output with `include_schema!`.

This example uses codegen and client 0.9 with the owned `rp-postgrest` 3.0 runtime.
Regenerate Rust bindings when upgrading. The generator emits schema-qualified finite field and FK lookups.
The committed format 3 snapshot supports offline builds. Regenerate older snapshots with the standalone codegen CLI.

Run without a database:

```sh
cargo run -p rp-supabase-codegen-example --offline
```

Cargo's offline flag requires cached dependencies. The executable checks omitted identity/default
fields, explicit null, query-first decoding, exact numeric values, and nested relationship shapes.

The executable uses `select!` with a function-local row import instead of a local `Message` DTO.
Typed column markers check filters. Generated payloads check insert and update fields.
`fetch()` infers the selected response type and checks HTTP errors before decoding JSON.

`src/relationship_projections.rs` contains mixed scalar, nested, inner, and predicate-only projections.
Relationship handles check child columns and selected projection ownership.
Compiler consumers cover local and generic selections, renamed runtime paths, identifier hygiene, strict nullable/duplicate keys, wrong edges/children/owners, inner and empty embeds, locked reselection, shared DTO contracts, and paged-read mutation rejection.

`src/gaps.rs` defines one `Content` DTO for both generated `articles` and `videos` relations.
Its permanent live scenario covers typed ordering, borrowed IN values with commas/quotes/backslashes,
JSON text paths, inclusive pagination, server counts, minimal writes, and raw `Vec<Content>` decoding.
It also renders `schema::params` pairs without constructing a typed query and feeds them to the
owned builder. An application can pass those unencoded pairs to its own HTTP query serializer.
JSON/JSONB markers implement `JsonColumn`; unrelated columns do not.

Typed RPC `.fetch()` infers the generated return type and returns the canonical owned REST error.
HTTP 204 supports unit-valued void RPCs, not arbitrary empty-body JSON results. Consumer response
tests cover this contract; the live executable calls `echo_message`, not a void function.
The example retains numeric values as arbitrary-precision `serde_json::Number`, without an `f64` conversion.


Run consumer behavior and compiler rejection checks:

```sh
cargo test -p rp-supabase-codegen-example --offline
```

## Live scenario

Apply `smoke.sql` to a disposable PostgreSQL database. It creates message, relationship, `articles`,
`videos`, and `content_links` fixtures, plus deliberately mismatched content tables for compiler
contracts. The `excerpt` and `message_meta` functions take an unnamed `messages` row, so they become
computed fields on `messages` rather than RPCs.
It grants access for verification. Do not apply it to the repository's existing Supabase
database or a production project.

Expose `public` through PostgREST. Set these environment variables:

- `SUPABASE_CODEGEN_DATABASE_URL`, a database connection string used only by `build.rs`. Use verified TLS remotely. Add `sslmode=disable` for a trusted local database.
- `SUPABASE_CODEGEN_API_URL`, the PostgREST base URL. Use `https://PROJECT.supabase.co/rest/v1` for Supabase.
- `SUPABASE_CODEGEN_API_KEY`, optional Supabase API key.
- `SUPABASE_CODEGEN_ACCESS_TOKEN`, optional bearer access token.

Then run `cargo run -p rp-supabase-codegen-example`. Live introspection never falls back to the snapshot after an error.
The executable first checks direct and reverse FK relationships, billing/shipping ambiguity,
PK/UNIQUE reverse to-one shapes, ordered composite FKs, nested filters, and empty-embed existence predicates.
It compares left and inner behavior. Run-specific cleanup removes relationship fixtures after recoverable errors.
Projected insert, update, and delete also run after scoped child filters lock the selection.

`gaps::live` runs next. It inserts run-specific articles and videos with minimal return, reads the
same shared DTO through both relations, checks exact counts and parameter rendering, and attempts
both cleanup deletes even after a recoverable scenario error. The article cleanup checks its affected
row count; the video cleanup requests minimal return.

It then inserts a projected message, reads it with a typed filter, updates its nullable field,
reads the view, calls the RPC, and deletes the row. It checks numeric precision through typed fetches.
It also selects, filters, and orders by the `excerpt` computed field. Computed fields are sent only
when selected, since PostgREST leaves them out of `*`.

Write payloads use `NonNullJson` for NOT NULL `json`/`jsonb` columns such as `metadata` and
`document`, so a JSON null is rejected before the request. Build one with `json!(...).try_into()`.

The scenario uses a 30-second timeout. A panic or timeout can leave inserted rows. Use a disposable database.

The fixtures grant access to `PUBLIC` and do not define production RLS policies. The scenario
requires visibility and write access to its inserted rows. It does not prove application RLS
correctness; RLS or permissions can change embed visibility and counts.
Live acceptance ran on Rust 1.85 with PostgreSQL 17 and PostgREST 16.2. Offline decoding and
HTTP-fixture tests are separate checks and are not evidence of a live server run.

Remote DDL does not trigger Cargo change detection. Change the tracked database environment variable, or register your migrations in `build.rs`, to request another introspection. Remove the database variable to return to the committed snapshot.
