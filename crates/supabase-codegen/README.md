# rp-supabase-codegen

[![crates.io](https://img.shields.io/crates/v/rp-supabase-codegen.svg)](https://crates.io/crates/rp-supabase-codegen) [![docs.rs](https://docs.rs/rp-supabase-codegen/badge.svg)](https://docs.rs/rp-supabase-codegen)

Generate Rust bindings for Supabase and PostgreSQL schemas in `build.rs`. No generator CLI is required.

The generator reads a versioned JSON snapshot or introspects PostgreSQL directly. It emits schema modules with table rows, insert and update payloads, computed fields, enums, composites, and named-argument RPC bindings.

## Offline builds

Keep a schema snapshot in source control. Ordinary builds need no database or credentials.

```toml
[dependencies]
rp-supabase-client = "0.14"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["arbitrary_precision"] }
# Add these when your schema has UUID or temporal columns.
uuid = { version = "1", features = ["serde"] }
chrono = { version = "0.4", features = ["serde"] }

[build-dependencies]
rp-supabase-codegen = "0.14"
```

Generated bindings only need the `schema` runtime. Applications with their own HTTP client and
response handling can use `rp-supabase-client = { version = "0.14", default-features = false }`.
That leaves out authentication and does not enable `serde_json/arbitrary_precision`.

```rust,no_run
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    rp_supabase_codegen::Generator::new()
        .from_snapshot("schema.json")?
        .write_to_out_dir("database.rs")?;
    Ok(())
}
```

```rust
pub mod database {
    rp_supabase_client::include_schema!("database.rs");
}
```

The generator registers the snapshot with Cargo's change detection. It writes only the requested file and leaves identical output unchanged. Rust formatting uses `prettyplease`, not an external formatter.

Schema crates can call `.reexport_macros()` and include their generated bindings at the crate root. Consumers then use `renamed_schema::select!(Row => { id })` and `renamed_schema::key!(id)` without naming the runtime dependency. The wrappers use `$crate` and a public hidden runtime reexport, so Cargo dependency aliases do not affect expansion. The same macros work inside the schema crate. `key!(type id)` also works.

See the [complete snapshot and executable example](../supabase-codegen-example). The public `model::Snapshot` format is version 4. Older snapshots require regeneration.

## Direct database introspection

Enable the `database` build-dependency feature. Database and TLS dependencies do not enter the application's runtime dependency graph.

```toml
[build-dependencies]
rp-supabase-codegen = { version = "0.14", features = ["database"] }
```

```rust,no_run
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=supabase/migrations");
    rp_supabase_codegen::Generator::new()
        .schemas(["public", "api"])
        .from_database_env("SUPABASE_CODEGEN_DATABASE_URL")?
        .write_to_out_dir("database.rs")?;
    Ok(())
}
```

Use a PostgreSQL connection string, not an anon key or service-role API key. Prefer a direct or session-pooler connection. Use a schema-owning role for complete metadata. Introspection reads catalogs in a read-only repeatable-read transaction. It does not read application rows or function bodies.

TLS verifies certificates and hostnames. The driver upgrades `sslmode=prefer` to `require`, preventing plaintext fallback. Use `sslmode=disable` only for a trusted local database. Connection errors do not print the connection string or raw server diagnostics.

Cargo cannot detect remote DDL. Track your migration directory, or add a refresh environment variable in your build script and change it after remote migrations. Do not assume every `cargo build` introspects again.

Live failures never fall back to cached metadata. Choose offline or live input explicitly.

### Creating a snapshot

Install the standalone CLI. It does not build the consumer crate or run its `build.rs`.

```sh
cargo install rp-supabase-codegen --features cli
export DATABASE_URL='postgresql://...'
rp-supabase-codegen snapshot write --out schema.json
rp-supabase-codegen snapshot check --out schema.json
```

Both commands default to `public` and `schema.json`. Repeat `--schema` to select more schemas. Use `--database-url-env NAME` for another environment variable. Prefer environment variables over `--database-url`, which can expose credentials in process listings and shell history.

`check` writes nothing. It reports schema drift and exits with status 1 on drift, missing snapshots, or invalid input. Review drift before running `write` with the same schema selection and output path. Snapshot acquisition does not require Rust type overrides.

The library also exports metadata without generating Rust bindings:

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    rp_supabase_codegen::Generator::new()
        .snapshot_from_database(&std::env::var("DATABASE_URL")?)?
        .write_to("schema.json")?;
    Ok(())
}
```

Do not write committed snapshots from an ordinary build script. `Bindings::snapshot()` also exposes typed metadata for tooling.

## Generated contracts

For `public.messages`, the generator emits:

- `public::tables::messages::Row`, with every readable field.
- `public::tables::messages::Insert`, with required fields and omittable default or nullable fields.
- `public::tables::messages::Update`, with omittable writable fields and `Default`.

Rows implement `schema::Relation` and `schema::Projection<Row>`. Each relation module exposes
`query(client)`. The query retains its relation and response type until decoding.
It consumes the client, so cloning remains explicit. Regenerate previously emitted Rust bindings
with current codegen before using the matching runtime. The snapshot format is version 4.
JSON and JSONB column markers, including domains over those types, implement `schema::JsonColumn`.

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use database::public::tables::messages;

let rows = messages::query(client.clone()).fetch().await?;
// rows has type Vec<messages::Row>.
# Ok(())
# }
```

### Typed projections and filters

Use a query-first selection for local results. Do not repeat Rust field types or a selection string:

```rust,no_run
# use rp_supabase_codegen_example::database;
# use database::public::tables::messages;
# async fn run(client: rp_supabase_client::Postgrest, message_id: i64) -> Result<(), Box<dyn std::error::Error>> {
use rp_supabase_client::{select, schema::Selection};
let selected = {
    use database::public::tables::messages::Row as Message;
    select!(Message => { id, body, note })
};
let rows = selected.query(client.clone())
    .eq(messages::columns::id, &message_id)
    .fetch()
    .await?;
# Ok(())
# }
```

Each generated `columns` marker records its owning relation, readable field type, non-null
filter type, and exact SQL name. The compiler rejects unknown columns, columns from another
relation, mismatched scalar values, and projections from another relation.

For a concrete generated relation, an unknown `select!` column diagnostic names its SQL schema
and table and lists every accepted Rust column identifier, including computed fields. The list
is sorted, uses raw spelling for keywords (for example, `r#type`), and remains the same through
function-local aliases, schema-crate dependency renames, and embedded selections. Generic relation
roots keep the runtime's key-bound lookup; their diagnostic cannot list a table that is not yet known.

This dispatch is generated as a public, hidden table-local lookup type and diagnostic trait.
They contain no row values, allocations, or selection state. Regenerate bindings when updating to
the matching runtime: `Relation` now requires its generated `ColumnLookup` associated type.

`select!` generates local records with the schema's types and nullability. Keep `projection!`
for named public results and reusable children, selecting them with `.select(schema::named::<_, Dto>())`.
Both use exact SQL keys, reject missing or duplicate selected fields, and ignore extra response keys.

Scalar comparisons support `eq`, `neq`, `gt`, `gte`, `lt`, and `lte`.
Values must implement `Display` and match the column's filter type through `Borrow`.
String columns accept `&str` without a caller allocation. Generated enums display their exact database labels.
Use `is_null(column)` for nullable fields, not `eq(column, &None)`.
The runtime escapes column identifiers and lets the HTTP client encode scalar values once.

### Computed fields

PostgREST treats a function that takes a row as a [computed field](https://docs.postgrest.org/en/stable/references/api/computed_fields.html).
The generator emits one when a function has exactly one input argument, the argument's type is the
row type of a selected table or view in the same schema, and it returns a single non-record value.
The argument may be unnamed. Set-returning functions, functions with more arguments, and functions
whose name matches a physical column are skipped.

A computed field gets a marker in the relation's `columns` module, so `select!`, `projection!`,
filters, ordering, and embedded filters use it like a column. Its value is always nullable,
because the function may return null for any row. It is not a field of `Row`, because PostgREST
leaves computed fields out of `select=*`. Insert and update payloads never include it.

```rust,no_run
# use rp_supabase_codegen_example::database;
# use database::public::tables::messages;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use rp_supabase_client::{select, schema::{Order, Selection}};
let selected = {
    use database::public::tables::messages::Row as Message;
    select!(Message => { id, excerpt })
};
let rows = selected.query(client.clone())
    .eq(messages::columns::excerpt, "Hello")
    .order(messages::columns::excerpt, Order::Desc)
    .fetch()
    .await?;
// Each row's `excerpt` field has type Option<String>.
# Ok(())
# }
```

A function with a named row argument also stays callable as an RPC. Use an unnamed argument to keep
it off `/rpc`. `.json_type(...)` and `.column_type(...)` accept computed-field targets, such as
`public.tables.messages.message_meta`.

`fetch()` infers `Vec<P>` for `P: Projection<R>`. `fetch_one()` infers `P` and uses PostgREST's
single-row response semantics. Both methods return the owned `rp_postgrest::Error` directly.
`Error::postgrest_body()` exposes the decoded code, message, details, and hint.
`postgrest_error()` exposes the canonical structured error. Response metadata retains the observed
status, headers, and effective URL. Malformed error envelopes and successful JSON decoding failures
have separate error variants.

One DTO can implement the projection contract for several relations:

```rust,no_run
# use rp_supabase_codegen_example::database;
rp_supabase_client::projection! {
    struct Content for [database::public::tables::articles, database::public::tables::videos] {
        id, name, owner_id
    }
}
```

The selected columns must exist in every relation with identical Rust value types and exact SQL
response keys. Unselected columns may differ. Relationship handles still enforce their source,
target, and selected child projection.

Typed queries support `order(column, Order::Asc)`, `order_with_nulls`, borrowed literal `in_`
values, and `json_text_eq(json_column, &["slug"], value)?`. JSON path keys are escaped
identifiers. Scalar comparison values remain literal; IN values use list-specific quoting.
`limit` and inclusive `range` produce a paged read query that cannot become a typed mutation.
`fetch_with_count(Count::Exact)` returns rows and the server total; `count(Count::Exact)` reads
the total without decoding rows. Missing or invalid totals are errors, never an invented zero.

`schema::params` renders the same unencoded query pairs without a `Postgrest` instance:

```rust,no_run
# use rp_supabase_codegen_example::database;
# use database::public::tables::messages;
# fn run(http: &rp_supabase_client::rp_postgrest::reqwest::Client, endpoint: &str, message_id: i64) {
use rp_supabase_client::schema::params;
rp_supabase_client::projection! {
    struct MessageSummary for database::public::tables::messages { id, body, note }
}
let pairs = [
    params::projection::<messages::Row, MessageSummary>(),
    params::eq(messages::columns::id, &message_id),
];
let request = http.get(endpoint).query(&pairs);
# }
```

It also provides typed comparison, null, IN, order, and JSON text-path helpers. Pass pairs directly
to your HTTP client's query serializer. Do not percent-encode them first.

### Typed relationships

Relationship markers identify an exact FK constraint. Forward markers use the normalized
constraint name. Reverse markers add the referencing table name, such as `orders_orders_customer`.
The generator emits markers for nonpartition base tables in the same explicitly selected schema.
Referenced types can add dependency schemas, but do not add endpoints.

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use database::public::tables::{addresses, customers, orders};

rp_supabase_client::projection! {
    struct AddressSummary for database::public::tables::addresses { id, label }
}
rp_supabase_client::projection! {
    struct OrderSummary for database::public::tables::orders {
        id,
        billing: embed(database::public::tables::orders::relationships::orders_billing, AddressSummary),
        shipping: embed(database::public::tables::orders::relationships::orders_shipping, AddressSummary, inner),
    }
}
rp_supabase_client::projection! {
    struct CustomerSummary for database::public::tables::customers {
        id,
        orders: embed(database::public::tables::customers::relationships::orders_orders_customer, OrderSummary),
        matching_orders: empty(database::public::tables::customers::relationships::orders_orders_customer),
    }
}

let rows = customers::query(client.clone())
    .select(rp_supabase_client::schema::named::<_, CustomerSummary>())
    .embedded(CustomerSummary::orders.then(OrderSummary::billing), |address| {
        address.eq(addresses::columns::label, "Main");
    })
    .embedded(CustomerSummary::matching_orders, |order| {
        order.eq(orders::columns::label, "open");
    })
    .exists(CustomerSummary::matching_orders)
    .fetch()
    .await?;
# Ok(())
# }
```

Aliases become response keys and typed filter handles. The compiler checks the source relation,
target relation, and exact selected child projection when composing handles.
An `empty(...)` selection has a handle but no decoded field.

To-one fields have type `Option<Child>`. Reverse fields have type `Vec<Child>`, unless their FK
column set exactly matches a recorded primary key or unique constraint, which produces `Option<Child>`.
These types remain conservative with `inner`. RLS or child filters can hide a non-null FK target.
Every visible selected embed key is required, including one whose value is null.

Child filters normally preserve parent rows. `inner` filters rows at the embed's parent level.
Use `exists(handle)` or `not_exists(handle)` for relationship existence or anti-existence.
Scoped filters support these predicates and further `.embedded(...)` scopes.
Any embedded filter or existence predicate locks the selection. Choose `.select(selection)` first.
Root filters and one typed mutation remain available after locking.

For UPDATE and DELETE, use root column predicates to constrain affected rows. Child filters shape returned representations.
PostgREST 16.2 rejects embed-alias existence predicates as DELETE row conditions.
The runtime reports the native execution error; it does not translate an embed into an FK null check.

Nested selections append into one parent buffer using `Projection<R>::SELECT_LEN` and
`write_selection`. Custom `Projection<R>` implementations must supply both for each relation.

Local nested selections infer the child relation from the FK:

```rust,no_run
# use rp_supabase_codegen_example::database::public::tables::customers;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use rp_supabase_client::{key, select, schema::Selection};
let selected = select!(customers::Row => {
    id,
    orders: orders_orders_customer {
        id,
        billing: orders_billing { label },
        shipping: inner(orders_shipping) { label },
    },
    matching_orders: empty(orders_orders_customer),
});
let billing = selected.orders.then(selected.orders.child.billing);
let rows = selected.query(client.clone())
    .embedded(billing, |address| {
        address.eq(billing.column(key!(label)), "Main");
    })
    .exists(selected.matching_orders)
    .fetch().await?;
# Ok(())
# }
```

The generator emits finite `ColumnByKey` and `RelationshipByKey` implementations directly
on each schema-qualified row. Keys encode the complete normalized Rust identifier as character types.
No global key registry, identifier hashing, or parsing of generated Rust is needed.
The resolved marker retains the original SQL response key, resource, and FK hint.
For renamed dependencies, configure the generator's runtime path and pass `runtime = path;` to `select!` and `key!`.



### Typed writes and raw queries

Pass generated payloads to `insert(&Insert)` or `update(&Update)`.
The builder handles serialization and reports failures when executing the query.
Only base tables implement `schema::WritableRelation`. Views do not expose typed writes.
A query can choose only one mutation. Projections also control returned write representations.
Write `fetch()` requests a representation. Write `execute()` requests minimal return without JSON
decoding, and `execute_with_count(Count::Exact)` returns the affected-row count.

Call `into_raw()` for arbitrary expressions, relationships outside the captured FK graph,
bulk writes, or comparisons whose mapped types do not implement `Display`.
This drops typed guarantees. The owned builder's `fetch::<Vec<MessageSummary>>()` still checks
HTTP status and decodes the named DTO through the same canonical error path.

### Omission and null

`schema::Field<T>` has `Omit` and `Value(T)`. Nullable writes use `Field<Option<T>>`:

| Rust value | Request |
| --- | --- |
| `Field::Omit` | No key. |
| `Field::Value(None)` | Explicit JSON null. |
| `Field::Value(Some(value))` | Explicit value. |

Non-null writes use `Field<T>`, so they cannot send null. Generated serializers omit `Omit` fields. Serializing `Omit` without the containing-field skip attribute returns an error.

Generated expression columns and ALWAYS identity columns appear only in rows. BY DEFAULT identities remain optional on insert and writable on update. Domain defaults and not-null constraints participate in insert requirements.

The generator resolves each relation column's read nullability and write obligation once.
Rows, column marker value types and nullable capabilities, insert/update fields, and automatic
`Default` derives consume that same private contract. `Insert` derives `Default` only when every
writable field is omittable; `Update` always does.

Bulk inserts with different omitted keys require care. PostgREST's `Prefer: missing=default` controls missing-key defaults in bulk requests. Generated omission alone does not change server preferences.

### Types

Integer widths match PostgreSQL. UUID and temporal columns use `uuid` and `chrono`. Timestamps with time zones use `DateTime<FixedOffset>`. JSON columns use `serde_json::Value` in rows.

Writes to `NOT NULL` JSON and JSONB columns use `schema::NonNullJson`, and strict non-null JSON RPC arguments do too. PostgREST turns a JSON null in the request body into SQL `NULL`, which the constraint rejects. Build one with `NonNullJson::try_from(value)` or from a `serde_json::Map`. Rows keep `serde_json::Value`, because a `NOT NULL` column can still store the JSON value `null`. Columns with a `json_type`, `column_type`, or type override keep that type.

Numeric columns use `serde_json::Number`. Enable `arbitrary_precision` for schema-only runtime use or decoding outside the client. The default client feature enables it for typed fetches. Do not convert exact numerics through `f64`. Non-finite numeric values need a custom mapping because they are not ordinary JSON numbers.

Bytea, network, interval, range, geometric, and text-search columns use their JSON string representation. SQL bytea does not map to a JSON byte array.

`schema::Array<T>` preserves null elements and variable rank with `Elements(Vec<Option<T>>)` and `Nested(Vec<Array<T>>)`. Use another outer `Option` for a nullable column. PostgreSQL does not enforce declared array dimensions. For JSON-valued array elements, JSON itself does not distinguish nested SQL arrays from JSON arrays stored as elements.

Enums preserve exact database labels with Serde renames. Composites honor snapshot field nullability. Live introspection defaults composite fields to nullable unless a type comment declares `@not_null id, name`. Domains preserve their qualified identity for overrides and otherwise resolve to their base type. Unknown types fail generation and require `type_override`.

Validated string membership CHECK constraints, including PostgreSQL's normalized `= ANY (ARRAY[...])` form, generate enums in `public::enums`, named after the table and column. Compound predicates, arbitrary casts and nonliteral values remain their SQL base types. Enums support Serde and `Display` for filters. `.column_type("public.check_probe.author_kind", "::domain::AuthorKind")` replaces a column's base Rust type across Row, Insert, Update and column markers without changing omission or nullability. The canonical `public.tables.videos.author_kind` target also works. Unknown columns or invalid Rust types fail generation.

Relationship canonical names remain unchanged. Single-column forward links expose the FK column name without a trailing `_id`, when unambiguous. Reverse links expose the source table name when unambiguous. `.relationship_alias("public.tables.orders.relationships.orders_customer_fkey", "buyer")` adds a checked custom alias. Alias markers retain the canonical marker's cardinality and exact PostgREST constraint hint. Unknown targets and alias name collisions fail generation.

Use `.json_type("public.functions.fetch_outcome.Returns", "crate::Outcome")` for typed JSON RPC results. Table columns accept `public.tables.content.document` or `public.content.document`; composite fields use `public.composites.JsonInfo.data`, RPC inputs use `public.functions.fetch_outcome.Args.audience`, and OUT fields use `public.functions.outcome_records.Record.data`. Targets must have a JSON/JSONB type, a domain over JSON, or an array of JSON. Custom Serde types bind in the root prelude scope. Generated fields retain SQL `Option`, `Array`, omission and set-returning wrappers, so plain fetches decode the custom type directly.

### Functions

Named-object RPCs emit `public::functions::<name>::Args`, `Returns`, and `Function`. Overloads use deterministic numbered modules, such as `lookup_0` and `lookup_1`, while retaining the original RPC name.

By default, non-default arguments use `Option<T>` because PostgreSQL functions can accept null. `.strict_args()` makes them `T` globally; `.strict_args_for("public.functions.publish_post")` does so for one generated function module. A function comment such as `@nullable p_revision` opts an argument back into `Option<T>` and persists that contract in the snapshot. Unknown annotation argument names fail introspection. Default arguments always use `Field<Option<T>>`, preserving omission versus explicit null. Custom JSON and SQL type mappings follow the same policy. Scalar results are nullable. Set results use vectors. Non-set composite and OUT results use a single struct. OUT and TABLE fields have a generated `Record` type, including singleton OUT and INOUT results.

```rust,no_run
# use rp_supabase_codegen_example::database;
# use rp_supabase_client::schema;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
let echoed = schema::rpc::<database::public::functions::echo_message::Function>(
    client.clone(),
    &database::public::functions::echo_message::Args {
        message: Some("hello".to_owned()),
    },
).fetch().await?;
# Ok(())
# }
```

The generated function marker determines the return type, so no manual decode or result annotation
is needed. RPC construction defers argument serialization errors until execution and does not add
relation selection or single-row semantics. PostgreSQL `void` emits `Returns = ()`; an actual
HTTP 204 decodes as unit. Empty bodies are not treated as successful row or scalar JSON.

PostgREST cannot distinguish every SQL overload by its JSON argument names. Generated overload types do not change that server limitation.

## Custom derives, attributes, and preludes

```rust
let generator = rp_supabase_codegen::Generator::new()
    .prelude("use ::std::string::String as DatabaseText;")
    .type_override("pg_catalog.text", "DatabaseText")
    .derive("PartialEq")
    .attribute("#[allow(dead_code)]")
    .type_attribute(
        "public.tables.messages.Insert",
        "#[derive(::typed_builder::TypedBuilder)]",
    );
```

Global derives and attributes apply to generated data structs and enums. Per-type attributes apply only to their named target. Use per-type attributes for macros that cannot operate on enums. The executable example compiles and uses `TypedBuilder` on its insert payload.

Target paths use generated Rust names. Keyword modules use raw spelling, such as `public.tables.r#type.Insert`. Record-result targets end in `.Record`. Unknown targets and invalid Rust syntax fail before output is written.

Prelude items appear at the generated root. Nested modules import their parent, so custom types remain available. `schema::prelude` exports runtime helpers and `include_schema!`. Use the generated schema modules explicitly to avoid cross-schema name collisions.

`runtime_path("::renamed_client::schema")` supports a renamed client dependency. Generated Serde derives and helper attributes require the normal `serde` dependency name.

## Scope and safety

View and materialized-view bindings are read-only. Ordinary views infer non-null direct base-column projections from a single base table, without joins, CTEs or grouping. Materialized views remain nullable because their stored rows can predate current base constraints. View comments and RPC record function comments can declare `@not_null field, other_field`. Snapshots retain these contracts. `.not_null("public.functions.publish_post.Record", ["post_id", "created"])` sets the same contract in the builder. View targets use `public.tables.view.Row`; composite targets use `public.composites.JsonInfo`. Unknown fields and targets fail generation.

The named-object RPC generator excludes unnamed input arguments, trigger functions, polymorphic pseudotypes, and dynamic records without named output fields. Those need different request bodies or explicit application-specific contracts. An unnamed row argument can still make the function a computed field.

Bindings do not validate CHECK constraints, enforce RLS, grant permissions, or type-check arbitrary PostgREST expressions. Computed relationships (row-returning functions used as embeds), view, self-referential, cross-schema, and partition-child relationships stay raw. Many-to-many queries can use explicit nested junction relationships; no direct shortcut is inferred.

## Verification example

```sh
cargo run -p rp-supabase-codegen-example
```

This compiles a real `build.rs` consumer and exercises generated serialization and decoding offline. For live verification, apply its `smoke.sql` to a disposable database, expose `public` through PostgREST, and set `SUPABASE_CODEGEN_DATABASE_URL` and `SUPABASE_CODEGEN_API_URL`.

The [research note](../../docs/schema-generation-research.md) records source comparisons and design decisions.
