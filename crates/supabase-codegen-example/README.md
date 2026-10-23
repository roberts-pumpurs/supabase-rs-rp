# Schema bindings example

This crate shows the typed query workflow end to end. `build.rs` reads the committed schema snapshot, `schema.json`, and generates Rust types for its tables, columns, relationships, and functions. `src/main.rs` includes them with `include_schema!` and queries with `select!`. The selected fields define each result type, so the example has no hand-written response structs.

## Run it without a database

From the repository root:

```sh
cargo run -p rp-supabase-codegen-example
```

Add `--offline` if Cargo already has the dependencies cached.

Without live settings, the program generates bindings from the snapshot, builds an insert payload, and decodes a fixture JSON response into a `select!` result. It makes no network requests.

To see the compiler check your queries, edit the `select!` call in `offline()` in `src/main.rs`. Remove `amount`, rebuild, and the line that reads `row.amount` no longer compiles.

## What to read

- `src/main.rs`: `select!`, typed filters, insert and update payloads, a view, a computed field, and a typed RPC call.
- `src/relationship_projections.rs`: selections that include related rows through foreign keys.
- `src/gaps.rs`: one shared struct, defined with `projection!`, read from two tables (`articles` and `videos`).
- `build.rs`: generator settings, including a prelude type alias, a text type mapping, and an extra derive on one generated struct.

A few details the example relies on:

- Generated write payloads separate "omit this field" from "send null". Insert and update payloads reject a JSON `null` for `NOT NULL` json or jsonb columns before the request goes out. Build one with `json!(...).try_into()`.
- Numeric columns keep their full precision through `serde_json`'s `arbitrary_precision` feature.
- `fetch()` infers the result type from the selection or the generated function, and checks the HTTP status before it decodes JSON.

## Tests

```sh
cargo test -p rp-supabase-codegen-example
```

The tests decode fixture responses and run compiler checks. `tests/compile_fail.rs` holds code that must not build, such as an unknown column, a filter value of the wrong type, or a relationship used from the wrong table.

## Live scenario

The live scenario reads and writes through a real PostgREST server. Use a throwaway database. `smoke.sql` grants access to `PUBLIC` and defines no row-level security policies, so do not apply it to this repository's Supabase database or to a production project.

1. Apply `smoke.sql` to a disposable PostgreSQL database. It creates the tables, a view, and the functions the scenario uses.
2. Expose the `public` schema through PostgREST.
3. Set these environment variables:
   - `SUPABASE_CODEGEN_DATABASE_URL`: a database connection string, used only by `build.rs`. Use verified TLS for a remote database. Add `sslmode=disable` for a trusted local one.
   - `SUPABASE_CODEGEN_API_URL`: the PostgREST base URL. For Supabase, use `https://PROJECT.supabase.co/rest/v1`.
   - `SUPABASE_CODEGEN_API_KEY`: optional Supabase API key.
   - `SUPABASE_CODEGEN_ACCESS_TOKEN`: optional bearer access token.
4. Run `cargo run -p rp-supabase-codegen-example`.

The program inserts a message, reads it back with a typed filter, updates it, reads a view, calls an RPC, and deletes the row. It also checks relationship queries, reads the shared struct from both tables, and selects, filters, and orders by a computed field. It removes the rows it created, also after most errors. A panic or the 30-second timeout can leave rows behind.

The scenario does not test your row-level security. Policies and permissions can change which rows and related rows a query returns. The live check last ran on Rust 1.85, PostgreSQL 17, and PostgREST 16.2.

## Schema changes

With `SUPABASE_CODEGEN_DATABASE_URL` set, `build.rs` reads the live schema. If that fails, the build fails. It never falls back to the snapshot. Cargo does not notice schema changes on a remote database, so change the tracked environment variable, or register your migrations in `build.rs`, to generate again. Unset the variable to go back to the committed snapshot.
