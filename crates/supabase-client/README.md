# rp-supabase-client

[![crates.io](https://img.shields.io/crates/v/rp-supabase-client.svg)](https://crates.io/crates/rp-supabase-client) [![docs.rs](https://docs.rs/rp-supabase-client/badge.svg)](https://docs.rs/rp-supabase-client)

Supabase authentication and query-first typed PostgreSQL queries. Uses the workspace-owned [rp-postgrest](../postgrest/README.md). Raw and typed requests share its checked execution, JSON decoder, and flat error type.

```toml
[dependencies]
rp-supabase-client = "0.15"
```

Use [rp-supabase-codegen](../supabase-codegen/README.md) in a build script to generate relation, column, relationship, payload, and function markers. The snippets below assume the generated `database` module from the [complete example](../supabase-codegen-example/README.md). Substitute your own generated names and value types.

## Query-first selections

Regenerate bindings with the matching codegen release. Describe the fields at the query instead of declaring a DTO for every local result:

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use rp_supabase_client::{key, select};
use rp_supabase_client::schema::Selection;

let selected = {
    use database::public::tables::orders::Row as LocalOrder;
    select!(LocalOrder => {
        id, label,
        billing: orders_billing {
            label, country: address_country { name },
        },
        shipping: orders_shipping { label },
    })
};
let country = selected.billing.then(selected.billing.child.country);
let rows = selected.query(client.clone())
    .embedded(selected.billing, |billing| {
        billing.eq(selected.billing.column(key!(id)), &10);
    })
    .embedded(country, |child| {
        child.eq(country.column(key!(name)), "UK");
    })
    .fetch().await?;
# Ok(())
# }
```

The FK determines each child relation. The result has ordinary named Rust fields with owned values. Filtering `billing.id` does not add that column to the response selection. `selected.billing.child.country` is relative to billing; `.then` composes a path from the root.

Function-local imports, concrete aliases, and bounded generic root/DTO types work. `key!(type id)` supplies the lookup-name type for a generic `ColumnByKey` bound. Keys encode the complete Rust identifier without hashing. Lookup uses the exact generated row type, including its schema, and rendering retains the exact SQL name.

An unknown column is one compile error at the written name. A concrete generated root,
including a type alias or nested relation, names the SQL table and lists its valid columns:

```text
error[E0277]: no column `title` on table `public.messages`
 --> tests/ui/select_unknown_column.rs:6:35
  |
6 |     let _ = select!(Root => { id, title });
  |                                   ^^^^^ unknown column
  |
  = note: valid columns: amount, body, created_at, excerpt, id, message_meta, metadata, mood, note, tags
```

Generic roots retain a readable unknown-column error and point to the generated `columns` module.
`column(key!(typo))` also names the key without printing character tuples.
Missing result fields list only selected fields, not the private selection marker.

A relationship typo reports ``no relationship `name` on `Row` `` the same way. If another item in the crate graph has the same name as the typo, rustc prefixes the name with the enclosing function, for example `main::title`.

Use `billing: inner(orders_billing) { label }` for an inner embed. `matching: empty(orders_billing)` creates a predicate-only handle without a decoded field. Named child reuse uses `billing: embed(orders_billing, AddressSummary)`, optionally with `inner` as the third argument.

For a renamed Cargo dependency, pass `runtime = renamed_client;` before the root type. Use the same prefix in `key!`. Generated bindings use `Generator::runtime_path("::renamed_client::schema")`.

Selections and inline handles are copyable zero-byte values. Each macro expansion has a distinct owner. Shared named child descendants use `selected.billing.then(AddressSummary::country)` or `AddressSummary::country` inside the billing filter scope. They do not expose `.child.country`. Paths containing named DTO handles retain their stored alias strings.

Missing selected fields fail decoding, including nullable fields. Explicit null is accepted where the schema permits it. Duplicate selected keys fail; extra keys are ignored. Named `projection!` and query-local `select!` compile through the same streaming typed decoder and exact-capacity field/embed renderer, without an intermediate JSON map. Neither local selection nor named child reuse requires `Debug` or `Serialize`. Generated record implementations for those traits depend on the selected values.

Keep named DTOs for reusable children and stable public return types. Move local result fields into an application DTO when needed; no JSON conversion is required.

## Shared projections and typed reads

A projection defines both the selection and response field types. `Projection<R>` is parameterized by the relation, so one DTO can implement several relation contracts:

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use database::public::tables::{videos, articles};
use rp_supabase_client::projection;
use rp_supabase_client::schema::{Count, Nulls, Order, named};

projection! {
    struct Content for [database::public::tables::articles, database::public::tables::videos] {
        id,
        name,
        owner_id,
    }
}

let names = ["search", "storage"];
let page = articles::query(client.clone())
    .select(named::<_, Content>())
    .in_(articles::columns::name, names)
    .order_with_nulls(articles::columns::name, Order::Asc, Nulls::Last)
    .order(articles::columns::id, Order::Desc)
    .json_text_eq(articles::columns::document, &["slug"], "abc123")?
    .range(0, 24)
    .fetch_with_count(Count::Exact)
    .await?;
let same_dto = videos::query(client.clone())
    .select(named::<_, Content>())
    .limit(10)
    .fetch()
    .await?;
println!("{} rows, {} total", page.data.len(), page.count);
# Ok(())
# }
```

The shared macro emits one DTO and a `Projection<R>` implementation for each relation. Every selected field must exist on each relation with exactly the same Rust value type and SQL response key. Unselected fields need not match. Selection preserves exact SQL response keys, including renamed identifiers. Missing selected fields are decoding errors even when their type allows null; extra fields are ignored. Named projections retain their attributes, visibility, and DTO-owned relationship handles. `projection!` resolves its runtime through `$crate`, including renamed dependencies; the hidden proc-macro support is version-owned implementation, not additional caller syntax.

### Shared filter keys

Append a `filters` block after a shared projection's selected fields:

```rust,no_run
# use rp_supabase_codegen_example::database;
# use rp_supabase_client::projection;
projection! {
    struct Content for [database::public::tables::articles, database::public::tables::videos] {
        name,
    } filters {
        content_id: [id, owner_id],
    }
}
```

Each key maps one generated column identifier per relation, in the order of the `for [...]` list. Here `Content::content_id::<articles::Row>()` names SQL `id`, while `Content::content_id::<videos::Row>()` names SQL `owner_id`. The compiler requires identical decoded value types and non-null filter types across the mappings. SQL names may differ. Unknown columns, missing mappings, duplicate keys, and use on another relation fail compilation.

Filter keys are not DTO fields and do not change the selection or decoder. The example selects only `name`. Use the associated marker with any existing typed column helper:

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use database::public::tables::{videos, articles};
use rp_supabase_client::{key, projection};
use rp_supabase_client::schema::{Column, FilterColumn, Relation, SharedFilter, named, params};
# projection! {
#     struct Content for [database::public::tables::articles, database::public::tables::videos] {
#         name,
#     } filters {
#         content_id: [id, owner_id],
#     }
# }

fn content_filter<R: Relation>(id: i64) -> params::QueryPair
where
    Content: FilterColumn<key!(type content_id), R>,
    SharedFilter<Content, key!(type content_id), R>: Column<Relation = R, Filter = i64>,
{
    params::eq(Content::content_id::<R>(), &id)
}

let pair = content_filter::<articles::Row>(7);
let rows = videos::query(client)
    .select(named::<_, Content>())
    .eq(Content::content_id::<videos::Row>(), &7)
    .fetch().await?;
# Ok(())
# }
```

`key!(type content_id)` names the key in generic bounds without a runtime discriminator. `SharedFilter` is a zero-sized relation-specific column marker. Nullable and JSON mappings retain their corresponding column capabilities for each relation. The `filters` block is available only with the shared `for [...]` grammar.

Column markers enforce relation ownership and scalar filter types. String columns accept borrowed `str`; nullable comparisons take a non-null value. `is_null(column)` requires a nullable column. Typed `in_` accepts borrowed scalar values and quotes/escapes them in list context. `order` composes multiple terms; `order_with_nulls` accepts `Nulls::First` or `Last`. `json_text_eq` requires a JSON/JSONB column and a nonempty key path. It escapes path identifiers and leaves the scalar value literal; an empty path returns a configuration error.

`limit` and inclusive `range` transition a read into `Paged`. Paged queries retain selection, filters, ordering, fetch, and counts, but have no insert/update/delete methods. This prevents response pagination from being mistaken for a safe mutation limiter. Ordering alone does not prohibit writes. `limit(0)` requests zero rows.

`fetch()` returns `Vec<P>`. `fetch_one()` returns `P` and requests PostgREST's single-object cardinality semantics, not the first row of an array. Read and Paged queries expose `.count(Count::Exact).await?` for a body-free total. `fetch_with_count` returns `Counted<Vec<P>>` with `.data` and `.count`. Planned and estimated count modes are also available. Missing or invalid server totals are errors, not zero. Counts preserve requested pagination.

## Minimal writes and raw decoding

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use database::public::tables::articles;
use rp_supabase_client::schema::{Count, named};
# rp_supabase_client::projection! {
#     struct Content for database::public::tables::articles { id, name }
# }
# let patch = articles::Update::default();

// `patch` is this generated table's Update payload.
let affected = articles::query(client.clone())
    .eq(articles::columns::name, "search")
    .update(&patch)
    .execute_with_count(Count::Exact)
    .await?;

articles::query(client.clone())
    .eq(articles::columns::name, "obsolete")
    .delete()
    .execute()
    .await?;

let rows = articles::query(client.clone())
    .select(named::<_, Content>())
    .into_raw()
    .fetch::<Vec<Content>>()
    .await?;
# Ok(())
# }
```

Write `.execute()` requests minimal return and does not decode a row body. `.execute_with_count(Count)` returns the affected-row total without fetching identifiers. Write `.fetch()` still requests and decodes representations. Generated insert/update payloads retain exact table ownership. Only base tables support typed writes; views support typed reads. A query can choose only one mutation.

`into_raw()` returns the owned `rp_postgrest::Builder` directly, applies the final projection, and drops typed guarantees. Use its public `fetch::<Vec<P>>()` for checked raw decoding; no separate raw decoder is needed. Use the raw builder for arbitrary expressions, unsupported relationships, or array/composite comparisons without `Display`. Raw `in_` takes grammar fragments; raw `in_values` takes literal list elements. Raw pagination does not promise safely limited mutations.

`schema::Field<Option<T>>` distinguishes omission, explicit null, and a value. `schema::Array<T>` preserves nullable elements and nested PostgreSQL arrays. The default `client` feature enables `serde_json/arbitrary_precision` for exact generated PostgreSQL numerics. With default features disabled, explicitly enable `serde_json/arbitrary_precision` when decoding numeric fields. Do not convert exact numeric fields through `f64` if their precision matters.

## Pure parameters for another HTTP client

`schema::params` needs no `Postgrest` instance. It produces unencoded `QueryPair` values with the same typed ownership and grammar as query methods:

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let http = rp_supabase_client::rp_postgrest::reqwest::Client::new();
use database::public::tables::articles;
use rp_supabase_client::schema::{Order, params};
# rp_supabase_client::projection! {
#     struct Content for database::public::tables::articles { id, name }
# }

let pairs = [
    params::projection::<articles::Row, Content>(),
    params::eq(articles::columns::name, "search"),
    params::in_(articles::columns::name, ["search", "storage"]),
    params::order(articles::columns::name, Order::Asc),
    params::json_text_eq(articles::columns::document, &["slug"], "abc123")?,
    params::limit(100),
    params::offset(0),
];
let response = http.get("https://example.supabase.co/rest/v1/articles")
    .query(&pairs)
    .send()
    .await?;
# Ok(())
# }
```

This example uses another Reqwest request directly; callers can pass pairs to another HTTP serializer. Do not percent-encode them first. The module also provides `neq`, `gt`, `gte`, `lt`, `lte`, nullable `is_null`, and `order_with_nulls`. Separate order pairs remain separate pairs; combine their terms into one order value if your transport/server requires multiple ordering terms. These helpers do not send requests, add auth/schema headers, or check responses.

For a table chosen at runtime, use `select` and literal-column `order_by` without generated relation markers:

```rust,no_run
# use rp_supabase_client::rp_postgrest::reqwest;
# use rp_supabase_client::schema::{Order, params};
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let http = reqwest::Client::new();
let table = "audit events";
let mut url = reqwest::Url::parse("https://example.supabase.co/rest/v1/")?;
url.path_segments_mut().unwrap().push(table);
let mut pairs = vec![
    params::select("*"),
    params::order_by("created_at", Order::Desc),
];
pairs.extend(params::range(100, 199)?);
let response = http.get(url).query(&pairs).send().await?;
# Ok(())
# }
```

`range(low, high)` returns offset and limit pairs for an inclusive range. It rejects reversed bounds and row-count overflow with `params::RangeError`. `limit(0)` requests zero rows. `order_by` escapes a literal column name; `select` accepts selection grammar. `order_by_with_nulls` adds explicit null placement.

Use `params::scope(&["tasks"], params::limit(5))` for a runtime embedded relation name or alias. It emits `tasks.limit=5`. Nested scopes take separate path segments. This limits selected child rows, not the root table, and does not add the embed itself.

### Runtime filters and keyset cursors

`params::filter("score", params::Op::Gt, 10)` returns an unencoded query pair. Column names are literal identifiers, not paths or expressions.

```rust
# use rp_supabase_client::schema::params;
# fn run() -> Result<(), Box<dyn std::error::Error>> {
let predicate = params::or(&[
    params::filter("score", params::Op::Gt, 10),
    params::and(&[
        params::filter("author_kind", params::Op::Eq, "organization"),
        params::filter("visibility", params::Op::Eq, "public"),
    ])?,
])?;
# Ok(())
# }
```

`or` and `and` quote scalar values in group context. They reject empty groups, non-filter pairs, and unsupported grammar with `CompositionError`. Supported pairs include scalar comparisons and nested groups, not IN lists or JSON paths.

`params::after(&[("org_id", 7), ("user_id", 42)])` builds an ascending lexicographic cursor. An empty cursor returns `None`. Order by all cursor columns in the same order, ascending. Use non-null cursor values and a unique final column to break ties. Heterogeneous values can use references to `dyn Display`.

## Typed RPC returns

```rust,no_run
# use rp_supabase_codegen_example::database;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
use database::public::functions::echo_message;
use rp_supabase_client::schema::rpc;

let args = echo_message::Args { message: Some("rpc example".into()) };
let result = rpc::<echo_message::Function>(client.clone(), &args).fetch().await?;
# Ok(())
# }
```

`args` is the generated function's `Args`; `result` is inferred as its `Returns`. Generated scalar, set-returning, composite, and void contracts decode their server representations. RPC does not require a relation projection or automatically add `select` or `single`. Serialization errors are deferred to execution.

Use `fetch_as::<Outcome>()` to decode JSON/JSONB directly into a caller-defined type. Use `single()` for server-enforced single-object mode:

```rust,no_run
# use rp_supabase_codegen_example::database::public::functions::echo_message;
# use rp_supabase_client::schema::rpc;
# type Outcome = String;
# async fn run(client: rp_supabase_client::Postgrest) -> Result<(), Box<dyn std::error::Error>> {
# let args = echo_message::Args { message: Some("rpc example".into()) };
let outcome = rpc::<echo_message::Function>(client.clone(), &args)
    .single()
    .fetch_as::<Outcome>()
    .await?;

rpc::<echo_message::Function>(client.clone(), &args)
    .execute()
    .await?;
# Ok(())
# }
```

Replace the function marker, arguments, and `Outcome` with your generated function and response type. `single()` sets the object Accept header but retains the generated return type. A set-returning function therefore needs `single().fetch_as::<Row>()`, not `single().fetch()`, to decode one row.

`execute()` checks status without reading or decoding a successful body. It preserves structured server errors and accepts empty or non-JSON success bodies. `fetch()` and `fetch_as()` still require the requested JSON shape. HTTP 204 can decode `()`; an empty HTTP 200 remains a JSON decoding error. `Rpc::into_raw()` is still available for raw protocol composition.

## Configured authentication

The default `anonymous_client` and `new_authenticated` constructors build a fallible default transport. That transport follows up to 10 redirects, and only to the same origin (scheme, host, port). Their `_with_client` variants reuse a caller's pool and policies. The authenticated stream shares that transport for REST, login, and every refresh.

```rust
use std::time::Duration;
use rp_supabase_client::{anonymous_client_with_client, new_authenticated_with_client};
use rp_supabase_client::rp_postgrest::reqwest;
use rp_supabase_client::rp_supabase_auth::{
    jwt_stream::SupabaseAuthConfig, types::LoginCredentials, url::Url,
};

fn configured() -> Result<(), Box<dyn std::error::Error>> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let url = Url::parse("https://example.supabase.co/")?;
    let _anonymous = anonymous_client_with_client("public-key".into(), &url, http.clone())?;
    let config = SupabaseAuthConfig {
        api_key: "public-key".into(), url,
        max_reconnect_attempts: 5,
        reconnect_interval: Duration::from_secs(3),
    };
    let credentials = LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());
    let _stream = new_authenticated_with_client(config, credentials, http)?;
    Ok(())
}
```

Each authenticated stream item is a `Result<(Postgrest, AccessTokenResponseSchema), SupabaseClientError>`. Consume it with `futures::StreamExt` to receive refreshed clients. API-key, JSON, and bearer headers coexist with caller default headers. Auth credentials apply per request rather than replacing shared bearer defaults. `SupabaseAuthConfig` has no HTTP-client field.

The crate reexports `rp_postgrest`, `Postgrest`, and `Error`. The default `client` feature includes authentication; `default-features = false` retains the generated-schema/query runtime without auth helpers. That is not a no-HTTP or `no_std` mode. See the raw crate's [TLS policy](../postgrest/README.md#install-and-configure) for transport feature configuration.

## Relationships, RLS, and errors

Single-relation `projection!` continues to support `alias: embed(RelationshipMarker, Child)` and its `inner` form. `alias: empty(RelationshipMarker)` selects a predicate-only embed. Generated direct, reverse, unique, composite, and nested FK relationships preserve source/target ownership. Selected handles compose with `.then(...)` only when the selected child and relation chain match. A shared DTO does not loosen these checks.

`embedded(handle, |child| { ... })` scopes filters to the selected relationship. `exists` and `not_exists` test related-row existence, including empty embeds. Embedded predicates lock the selection, so choose `.select(selection)` first. Root filters and one mutation remain available on non-paged locked queries.

To-one embeds decode as `Option<Child>` and to-many embeds as `Vec<Child>`. These types remain conservative with `inner`, because RLS and filters can hide rows. Ordinary child filters preserve parent rows; `inner` filters at the embed's parent level. For UPDATE and DELETE, constrain affected rows with root filters. Child filters shape returned representations. PostgREST 16.2 rejects embed-alias existence predicates on DELETE; the client reports the canonical server error and does not rewrite them into FK null checks. See [generated relationship contracts](../supabase-codegen/README.md#typed-relationships) for edge and naming rules.

Typed fetch, RPC, raw fetch, and count methods return the same flat `rp_postgrest::Error`. `error.postgrest_body()` exposes decoded code/message/details/hint; `postgrest_error()` exposes the canonical structured source. `status()`, `url()`, and `response_metadata()` retain observed HTTP metadata, including on successful-response decoding failures. HTTP 300 is an error for checked execution. Ambiguous embedding PGRST201 details can be a typed array of relationship descriptions, not only text. Malformed error-envelope bytes remain in the Decode source. See the [raw error guide](../postgrest/README.md#one-error-result).

## Migration from 0.14

- Regenerate bindings with the matching runtime, macro, and codegen versions.
- Value-form `key!(id)` includes a readable name marker. For local type annotations, use `Key<key!(type id), _>`.
- `key!(type id)` and generic `ColumnByKey<key!(type id)>` bounds stay unchanged.
- Handwritten `Relation` implementations must provide `type ColumnLookup = schema::selection::RelationToken<Self>;`.
- Generated relations provide their table-specific lookup. Local records still have movable named fields and preserve SQL serialization keys.
- Default features include `client` and `rustls`. For native TLS, disable default features and enable `client,native-tls`.

## Migration from 0.13

- Move `rp-supabase-client` and `rp-supabase-codegen` to 0.14 together.
- The re-exported `rp_supabase_auth` is 0.11 and uses simd-json 0.18. Move a direct simd-json dependency to 0.18.
- Rust 1.88 or later is required.

## Migration from 0.12

- Regenerate snapshots and Rust bindings with codegen 0.13. Snapshot format 4 adds computed fields.
- Generated writes to `NOT NULL` JSON/JSONB columns take `schema::NonNullJson`. Convert with `NonNullJson::try_from(value)?` or `NonNullJson::from(map)`.

## Migration from 0.11

- The re-exported `rp_supabase_auth` is now 0.10. Build credentials with `LoginCredentials::email(email, password)` or `LoginCredentials::phone(phone, password)` instead of the builder. See the [rp-supabase-auth README](../supabase-auth/README.md#migration-from-09).
- Regenerate Rust bindings with codegen 0.12.

## Migration from 0.10

- The re-exported `rp_supabase_auth` is now 0.9. Its request results are flat: `Response::json` returns `Result<T, AuthError>`, API errors are `AuthError::Api { status, error }`, and `RefreshStreamError` has one `AuthError` variant. See the [rp-supabase-auth README](../supabase-auth/README.md).
- `SupabaseClientError::RefreshStreamError` wraps the new `RefreshStreamError`. Match on its `AuthError` instead of the removed `Reqwest`, `JsonParse`, `SupabaseApiError`, and `ErrorResponse` variants.
- Regenerate Rust bindings with codegen 0.11.

## Migration from 0.8

- Regenerate Rust bindings with the current codegen. Snapshot format 3 requires regeneration of older snapshots.
- Replace `.select::<Dto>()` with `.select(named::<_, Dto>())`. Import `schema::named`. The old type-only method is removed.
- Prefer `select!(Row => { ... })` for local queries and use the returned selection's handles.
- Keep `projection!` for named/shared DTOs. Existing DTO-owned relationship constants still compose with local selections.

## Migration from 0.7

- Replace the old `postgrest` reexport with `rp_postgrest`, or use the direct `Postgrest`/`Error` exports. The owned dependency is rp-postgrest 3.0, with the normal `rp_postgrest` library name.
- Remove `PostgerstResponse`, `ResponseError`, `schema::QueryError`, and manual response-wrapper/nested-result decoding. Fetch now returns `Result<T, Error>` directly. Replace old execution-wrapper pattern matches with canonical `Error` handling and its getters. No old exports or aliases remain.
- Replace `query.into_raw()?` with `query.into_raw()`. Use `builder.fetch::<Vec<P>>().await?` for raw typed decoding instead of the removed raw-result/response helpers. Raw `execute` deliberately remains unchecked.
- Add `?` to direct `Postgrest::new`, client auth/header setters, and raw `.build()`. JSON serialization failures now live in the builder and emerge at build/execution.
- Implement custom projections as `Projection<R>` instead of using an associated `Relation`. Preserve `SELECT_LEN`, `write_selection`, and exact response-key decoding. Regenerate bindings with codegen 0.9. Custom embed paths must supply their `Source` relation.
- Use `schema::rpc::<Function>(client, &args).fetch().await?`; the generated `Returns` is inferred, and no relation-specific projection/single or separate response decode is required.
- Count preferences no longer force a one-row range. Add pagination explicitly or use typed `.count(Count)`/raw `.execute_count(Count)` for body-free totals.
- A paged typed read cannot become a write. Start a separate root-filtered mutation rather than treating read pagination as a mutation limit.
- Builders are not `Clone`; recreate queries from `Postgrest::clone()`. Pass literal resource names, not percent-encoded strings. Dot-only resources return an explicit configuration error.

The [raw 2.1 migration guide](../postgrest/README.md#migration-from-21) covers the remaining protocol changes.
