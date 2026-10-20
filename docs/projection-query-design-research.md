# Query-first projections research

Date: 2026-10-06. Repository base: `21630f4` on `main`. Research only. The production projection interface remains unchanged.

## Question and decision

Must callers declare a projection DTO before writing a query? Can generated schema and relationship metadata provide a query-first interface like Bevy or TypeScript?

No upfront DTO is necessary for local queries. The current macro has useful decoder and relationship checks, but its mandatory named-type declaration is not the cleanest default interface.

Recommendation: make the selection determine its result type. Offer ordinary typed column tuples for small queries and a query-site selection macro for named, nested results. Keep explicit named projections for shared DTOs, public return types, and reusable contracts. Do not generate every possible field combination.

This is a design recommendation, not an implemented product interface. A compiled experiment proves the local-record mechanism only.

API compatibility is not a constraint. The user accepts a breaking change if it improves the product. Preserve wire behavior and type-safety contracts, not the current `.select::<P>()` spelling.

## Current crate and module structure

- `crates/supabase-codegen/src/emitter.rs` emits relation, column, relationship, row, payload, and function markers from versioned schema metadata.
- `crates/supabase-client/src/schema/query.rs:14-43` defines `Column::Relation`, `Column::Value`, exact SQL identifiers, and `Projection<R>` rendering plus deserialization.
- `query.rs:57-101` stores the selected projection in `Query<R, P, State, Selection>`. `.select::<P>()` requires a pre-existing `P: Projection<R>`.
- `crates/supabase-client/src/schema/projection.rs:28-110` generates a named struct, selection renderer, strict map deserializer, and projection-owned relationship handles.
- `crates/supabase-client/src/schema/relationship.rs:6-35` already computes child result wrappers through `Cardinality::Output<P>`. To-one is `Option<P>`; to-many is `Vec<P>`.
- `relationship.rs:36-96` checks selected owner, source, target, child shape, and alias path. These are stronger contracts than knowing the target table alone.

The implementation decodes directly into typed fields. Missing selected keys fail, including nullable fields. Unknown response keys are ignored. A replacement must preserve these properties.

## What Bevy actually does

The inspected release is Bevy ECS 0.19.1. Its [QueryData](https://docs.rs/bevy_ecs/0.19.1/bevy_ecs/query/trait.QueryData.html) has an associated `Item<'w, 's>` result. Component references, tuples, `Option`, and nested query data compose this type.

```rust,ignore
fn inspect(query: Query<(Entity, &Position, Option<&Velocity>), With<Player>>) {
    for (entity, position, velocity) in &query {
        // The tuple shape and component types are static.
        // Membership and optional component presence are runtime facts.
    }
}
```

A caller does not need a named query struct for a tuple. `#[derive(QueryData)]` is the named-field and reuse option. It generates nominal item structs such as `MotionItem`; it does not synthesize arbitrary fields from runtime data.

[QueryBuilder](https://docs.rs/bevy_ecs/0.19.1/bevy_ecs/query/struct.QueryBuilder.html) adds runtime filters and component access while retaining a statically supplied `D: QueryData`. Runtime component IDs can select dynamically inspected components, but results remain known wrappers such as [FilteredEntityRef](https://docs.rs/bevy_ecs/0.19.1/bevy_ecs/world/struct.FilteredEntityRef.html). They do not become newly typed `.field` properties.

Bevy also validates access at runtime. Conflicting mutable accesses can compile and then panic during system initialization. [QueryState transmutation](https://github.com/bevyengine/bevy/blob/v0.19.1/crates/bevy_ecs/src/query/state.rs#L682-L777) checks whether requested access fits existing access. This is not compiler inference of a new record.

Its [unsafe storage and borrowing implementation](https://docs.rs/bevy_ecs/0.19.1/bevy_ecs/world/unsafe_world_cell/struct.UnsafeWorldCell.html) supports references into concurrently accessed ECS storage. Our HTTP client returns owned JSON values. We need typed composition and response checks, not ECS pointers, access bitsets, or transmutation.

## Why TypeScript looks simpler

Supabase's [`select` result type](https://github.com/supabase/supabase-js/blob/4d23055805ce2cf853686b704cfaec1a701d29c6/packages/core/postgrest-js/src/PostgrestQueryBuilder.ts#L911-L960) passes the selection string through `GetResult`. A [template-literal parser](https://github.com/supabase/supabase-js/blob/4d23055805ce2cf853686b704cfaec1a701d29c6/packages/core/postgrest-js/src/select-query-parser/parser.ts) and [mapped result computation](https://github.com/supabase/supabase-js/blob/4d23055805ce2cf853686b704cfaec1a701d29c6/packages/core/postgrest-js/src/select-query-parser/result.ts) combine literal syntax with generated schema and relationship types.

This is real compile-time machinery. A widened runtime `string` explicitly reaches `ParserError<'Received a generic string'>` instead of exact selected-field inference. The runtime method can still accept and send that string. TypeScript does not infer an exact record from every dynamic query.

Rust can compute tuple output through [associated types](https://doc.rust-lang.org/reference/items/associated-items.html#associated-types). It cannot create new named struct fields through trait composition alone. [Macros](https://doc.rust-lang.org/reference/procedural-macros.html) can emit local structs and associated-type expressions, then let the compiler resolve field types. A macro does not need to inspect rustc's resolved schema types.

## Proposed ordinary Rust interface

Illustrative only:

```rust,ignore
let rows = articles::query(client)
    .select((articles::columns::id, articles::columns::name))
    .eq(articles::columns::owner_id, &owner_id)
    .fetch()
    .await?;

for (id, name) in rows {
    // id and name are inferred from generated Column::Value types.
}
```

A safe selection trait can associate an `Output` type. Column selections use `Column::Value`; tuples compose outputs; relationship selections recursively wrap child output with existing cardinality metadata. No lifetime GAT is needed for ordinary owned scalar output.

[Diesel's tuple selections](https://docs.diesel.rs/2.3.x/diesel/query_dsl/trait.QueryDsl.html#method.select) and [tuple implementation](https://github.com/diesel-rs/diesel/blob/v2.3.2/diesel/src/type_impls/tuples.rs) provide precedent for typed composition. Diesel's documented `load` examples still name a Rust decoding target. Our explicit `Column::Value` metadata can determine that target for supported column selections.

PostgREST returns JSON objects, not tuple arrays. Ordinary Serde tuple deserialization is insufficient. A tuple implementation must decode selected object keys directly, without an intermediate `serde_json::Value` or row map. This decoder is real implementation work, not a new type alias.

Avoid overlapping blanket implementations for arbitrary columns and tuples. Generated column implementations or a library-owned field-selection wrapper can keep trait implementations disjoint. Rust has no arbitrary variadic tuple generics; document bounded arities or support nesting.

## Proposed named, query-first interface

Illustrative only. The spelling is not an accepted interface:

```rust,ignore
let selected = select! {
    orders {
        id,
        label,
        billing: embed(orders::relationships::orders_billing => addresses) {
            id,
            label,
        },
    }
};

let rows = orders::query(client)
    .select(selected)
    .embedded(selected.billing, |billing| {
        billing.eq(addresses::columns::label, "headquarters");
    })
    .fetch()
    .await?;

for row in rows {
    println!("{}", row.label);
    if let Some(address) = row.billing {
        println!("{}", address.label);
    }
}
```

The selection declares its local root and child records at the query site. Fields use `<ColumnMarker as Column>::Value`. Child fields use the generated relationship's cardinality output. One selection determines exact wire keys, strict decoding, and relationship occurrence handles.

The proposed descriptor is zero-sized and copyable. It exposes typed handles such as `selected.billing`. Returning only a query would hide the local record name that the current interface needs for `ProjectionName::billing`.

The example names the child relation module explicitly. A relationship's associated `Target` is a row type, not a Rust columns module. Omitting `addresses` requires a generated field lookup contract. Do not assume a macro can recover that module through type reflection.

This removes upfront DTO declarations. It does not remove types or compiler checks. The implementation can reuse the current renderer and strict decoder while changing the selection API.

A constrained macro grammar must expose schema marker paths. A macro cannot recover a column type from an arbitrary fluent value expression. Keep filters, ordering, counts, and execution as normal typed builder methods. They do not need another query language.

## Compiled experiment

A throwaway consumer used the published client/codegen 0.8.0, the real committed schema snapshot, and Rust 1.99.0. An expression macro emitted a local `LocalResult` with the existing `projection!` implementation, then returned `.select::<LocalResult>()`.

It executed and checked:

- Inferred local result type with `.id` and `.name` field access.
- Real generated `Column::Value` field types.
- Selection grammar exactly `id,name`.
- Direct decoding through the current strict projection deserializer.
- Rejection of a missing selected key.

Observed output:

```text
Inferred local result type: alloc::vec::Vec<projection_inference_research::main::LocalResult>
Named field access, generated SQL selection, and strict JSON decoding pass without an upfront DTO declaration.
```

The experiment did not send HTTP requests. It did not prove nested macro ergonomics, tuple decoding, editor completion, compile-time cost, or a production replacement. Its code is temporary, not a shipped interface.

## Contracts the new interface must preserve

1. Exact source relation and column ownership, including shared DTO relations.
2. Exact SQL response keys and grammar escaping.
3. Missing selected keys remain errors. SQL null and an omitted selection are different states.
4. Conservative `Option<Child>` for to-one and `Vec<Child>` for to-many, including RLS-hidden children.
5. Relationship occurrence identity. Billing and shipping can target the same table but require different aliases and FK hints.
6. Selected-child scope for nested filters and path composition. Keep root typed ordering. `ScopedFilters` has no typed ordering methods today. Locking cannot be weakened by inferred records.
7. Read/write/Paged state rules, canonical errors, counts, minimal return, and raw escape behavior.
8. Direct typed JSON map decoding without hidden overfetch, dynamic maps, or per-field intermediate allocations.

Runtime filter values are compatible with fixed static output. A runtime boolean cannot change one Rust variable into unrelated record shapes. Keep a fixed explicit optional contract, branch over known alternatives, or use the existing explicit raw escape. Do not add fake defaults for missing fields.

## Tradeoffs and recommended direction

| Choice | Local query use | Named field access | Reusable public result type |
| --- | --- | --- | --- |
| Current named projection | Requires a separate declaration | Yes | Yes |
| Typed tuple selection | Query-first | Destructuring, not named fields | Explicit tuple/selection type |
| Query-site generated record | Query-first | Yes | Separate named DTO or generated item needed |
| Runtime arbitrary selection | Dynamic | Checked dynamic access only | Explicit dynamic contract |

Identical local macro invocations create distinct nominal Rust types. They cannot silently replace the shared `Content` DTO across articles and videos. Named projections remain useful when one type must cross function, crate, or API seams. Returning `impl Serialize` also hides fields from downstream callers, as the [opaque return-type rules](https://doc.rust-lang.org/reference/types/impl-trait.html#abstract-return-types) specify.

I recommend one selection-value interface with an associated output type. Both tuple composition and local named records should use that interface. Keep explicit named records for shared DTOs and public return types. Do not preserve `.select::<P>()` solely for compatibility.

Use query-site named selections as the application default if nested handles and editor completion work well. Tuples provide the small ordinary-Rust alternative. Users should not need both forms for a simple query.

A thin macro around the existing `Projection` proves scalar ergonomics. It does not prove a complete nested interface. Reuse the strict decoder, but do not let the old trait prevent a cleaner selection model. Do not create a second decoder engine.

Before implementation, check child column lookup, billing/shipping aliases, caller-accessible filter handles, shared child DTOs, missing-versus-null decoding, and public return types. Compare editor completion and error messages, not only syntax length. Allocation and compile-time performance remain unmeasured.

## Toolchain update

The [official stable distribution manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml) identifies Rust 1.99.0 dated 2026-10-01. Rustup installed `rustc 1.99.0 (b940084d7 2026-09-28)`.

Development and CI now pin 1.99.0. Published crate minimum-version declarations remain 1.85. Updating the development compiler does not require dropping consumer support.

New style-only Clippy rules retain established bounds, patterns, module layout, test names, doc labels, and empty-result assertions. Small source fixes remove redundant syntax and future-incompatible example macro use. Scoped lint expectations document existing formatting failure semantics, an impossible UTF-8 failure, and Tokio macro runtime construction. The nested realtime condition retains Rust 1.85 support.

Final formatting, workspace Clippy with `-D warnings`, and Rust 1.85.1 all-feature/all-target compatibility checks pass.

Two subagents review the design and verify the final source. Final Rust 1.99 verification passes 134 workspace tests and 9 doctests. No-default-feature checks pass 36 native-client tests and doctests, plus 8 Supabase-client tests. The actual offline example runs generated-binding serialization and nested relationship rendering. These checks do not exercise a live database.

## Further compiled prototypes

The next pass validates child lookup, caller handles, RLS, and views. Three implementation subagents build independent slices. A separate developer-UX subagent writes a consumer and checks compiler diagnostics. A reviewer checks ownership, wire behavior, and server claims. The parent combines the slices in a live inferred-query program.

The runnable code is captured on local branch `prototype/query-first-20261006`, commit `b48fd05`. It is not pushed or added to the production workspace. The worktree remains at `/tmp/supabase-query-first-prototypes-20261006`. Its prototype README contains commands and evidence.

Run the complete experiment there:

```sh
bash crates/supabase-client/prototypes/query-first/run.sh
```

The runner needs Docker and Rust 1.99.0. It creates disposable PostgreSQL 17.11 and PostgREST 16.2 services on localhost ports 55439 and 55440. It captures live schema metadata, runs six consumer/runtime demos, checks module-level imports, and rejects 21 designated invalid programs. The observed full run passes. Its cleanup removes the containers, anonymous volumes, and network.

### Child lookup and relationship handles

Finite generated contracts resolve columns and FK keys on a relation:

```rust,ignore
trait ColumnByKey<K>: Relation {
    type Column: Column<Relation = Self>;
    const COLUMN: Self::Column;
}
trait RelationshipByKey<K>: Relation {
    type Edge: Relationship<Source = Self>;
}
```

Child lookup follows `Edge::Target`. No repeated child module path or Rust type reflection is needed. The prototype supplements real generated bindings with typed snapshot metadata and actual AST identifiers. Production should generate these mappings directly. This supplement validates one `public` schema, not arbitrary same-named relations across schemas.

The combined prototype returns a copyable selection descriptor. Callers use `selected.billing`, `selected.shipping`, and `selected.billing.child.country`. `.then` composes a root path. `.column(keys::id)` resolves a child column through the handle's target, even when that scalar is not selected. Descriptors and composed paths measure zero bytes.

The live inferred program uses a freshly captured schema snapshot. It sends authenticated queries for both tenants and decodes real responses into inferred named records. It exercises distinct billing/shipping hints, nested country filters, hidden children, empty child lists, parent existence, and filtering an unselected billing ID without adding it to the selection.

Compiler probes reject another invocation's handle, a wrong child DTO target, an unselected child, a foreign column scope, an uncomposed relative path, and reselection after filtering. Nominal ownership is per macro expansion, not per runtime evaluation of the same expression.

### RLS and views

Actual non-null FK scalars accompany absent visible children. Tenant 1 sees root IDs `[10, 11, 12]`; tenant 2 sees `[20, 21]`. Hidden targets physically exist, but return `None`. Visible child collections contain only allowed rows and can be empty.

Child filters preserve parents by default. A matching child plus `exists` retains `[10]` or `[20]`. `!inner` has the same parent-filtering effect in the exercised case. Missing target `SELECT` privilege instead produces HTTP 403 and SQLSTATE 42501.

The `security_invoker` view returns only the caller's tenant. The ordinary postgres-owned view deliberately exposes both tenants. Static result types do not certify policy isolation. PostgreSQL documents [owner versus invoker policy behavior](https://www.postgresql.org/docs/17/sql-createview.html) and [FK integrity bypassing RLS](https://www.postgresql.org/docs/17/ddl-rowsecurity.html).

The key-preserving view supports embedding on PostgREST, but the generator emits no corresponding relationship marker. The experiment labels its handwritten witness explicitly. An aggregate view without the needed keys rejects the attempted embed with `PGRST200`. [PostgREST view inference](https://docs.postgrest.org/en/stable/references/api/resource_embedding.html#foreign-key-joins-on-views) depends on projected base keys and view complexity.

View scalar nullability remains conservative. Generated views have no `WritableRelation`, although PostgreSQL reports the simple fixture views as updatable. These are generator capabilities, not guarantees about all server views.

### Independent developer-UX verdict

The evaluator recommends the query-first direction. The strongest interface combines the lookup prototype's terse field grammar with the handle prototype's selection descriptor. That combined terse spelling remains a recommendation, not an implemented macro.

No upfront DTO inventory is needed for local queries. Named child DTO reuse and public named results work. The public-result consumer moves owned fields without cloning or decoding through a dynamic JSON map.

Before default adoption:

- Function-local root and DTO imports must resolve at the invocation scope. Enclosing-module imports now work. Qualified paths remain a workaround.
- Bad-key selections need fewer cascades and narrower locations. The initial review observes 50 and 56 reported errors for bad column and FK keys.
- Relative child handles and root composition need clearer diagnostics. A reused `Shared<P>` child has no generated descendant handles.
- View relationship and mutation support must remain separate from server capabilities and RLS guarantees.
- Internal projection macro arms and generated-source AST parsing must not become unsupported production coupling.

The review also finds a raw identifier alias mismatch. The corrected prototype uses `type` consistently for `r#type` selection, filtering, and decoding. A real HTTP probe passes.

No editor completion or performance benchmark is claimed. The next pass below tests invocation-scope record generation and localized invalid-field diagnostics.

## Invocation-scope generation and diagnostic comparison

Prefer the constructor-checked macro design. Both implementations fix function-local imports, but the constructor design reduces diagnostic cascades and supports outer generic functions.

Two implementation subagents build competing macros. A separate developer-UX subagent writes a standalone consumer with renamed dependencies. A reviewer checks decoding, target bounds, ownership, and identifier hygiene. The parent runs both implementations against real RLS responses.

The updated runnable branch is `prototype/query-first-20261006`, commit `f1b3050`, in `/tmp/supabase-query-first-prototypes-20261006`. Production sources remain unchanged. Nothing is pushed.

The same runner now completes 14 positive demo configurations, a module-import compiler check, and 54 designated compile rejections. It captures fresh live metadata and exercises PostgreSQL 17.11 and PostgREST 16.2. The final full run passes and removes its containers, anonymous volumes, and network.

### Two implementations

`scoped` emits concrete record items directly inside the invocation block. Ordinary function-local root imports, concrete aliases, and named child DTO aliases resolve. Outer function generic parameters still fail with `E0401`, because nested items cannot capture them.

`checked` emits generic records parameterized by resolved field descriptors. Column and FK lookup happens in constructor expressions. The record, visitor, and renderer do not repeat unresolved schema lookup constraints. Child relation tokens use the FK's exact target.

The two macros expose the same terse grammar and typed handle interface:

```rust,ignore
use query_first_checked_prototype as selection_runtime;
use selection_runtime::Selection;

let selected = {
    use selection_runtime::database::public::tables::orders::Row as LocalOrder;
    use selection_runtime::AddressDto as Shipping;
    selection_runtime::select!(runtime = selection_runtime; LocalOrder => {
        id, label,
        billing: orders_billing {
            label, country: address_country { name },
        },
        shipping: embed(orders_shipping, Shipping),
    })
};
```

No child table modules or upfront record definitions are needed. Reused DTOs need `Projection<Target>`, not `Debug` or `Serialize`. Checked's generated record implementations for those traits are conditional.

### Compiler evidence

The representative failures select `addresses.name` under billing or use `orders.address_country`. Counts include compiler error diagnostics, not Cargo failure summaries.

| Implementation | Invalid column | Invalid FK |
| --- | ---: | ---: |
| First-pass lookup, recorded final baseline | 47 | 50 |
| Invocation-local concrete records | 35 | 35 |
| Constructor-checked generic records | 2 | 2 |

The independent consumer reproduces the 35-versus-2 result. Checked's first errors point at the offending `name` and `address_country` tokens. A second diagnostic remains macro-wide. Wrong shared DTO targets remain less localized; the independent consumer produces two errors with a coarse first span.

Both implementations reject foreign owners, wrong filter columns, malformed syntax, duplicate aliases, and reselection after locking. Checked's consumer also runs bounded generic root and generic shared-DTO functions. Scoped rejects outer generic capture.

### Identifier hygiene and behavior

Review finds valid `m` and `k` aliases colliding with decoder locals. A `__marker` alias also collides with an internal field. Function-local `String`, `Result`, trait names, and `Some`/`None`/`Ok`/`Err` names alter unqualified generated code.

The parent reproduces the checked failures. The independent consumer observes the same variant-capture defect in scoped. Indexed decoder slots, a collision-free marker, and absolute core/std paths fix the defects. Both final consumers pass those adversarial cases.

The consumers also pass owned field access after dropping the JSON input, nested shared DTOs, named result field moves, nullable scalar/child nulls, strict missing/duplicate keys, ignored unselected keys, raw identifier aliases, actual SQL column names, and exact nested filter pairs. Handles remain copyable and zero bytes, with nominal ownership per expansion.

Both new macros send real authenticated HTTP requests using function-local root aliases and the original deserialize-only shared DTO. Tenant roots remain `[10, 11, 12]` and `[20, 21]`. Hidden non-null FK targets remain `None`; hidden to-many children remain empty. Child existence retains `[10]` or `[20]`. Unselected scalar filters do not add fields to the selection.

Rust's [procedural macro hygiene rules](https://doc.rust-lang.org/reference/procedural-macros.html#procedural-macro-hygiene) explain the need for qualified generated paths. Its [nested item generic scope](https://doc.rust-lang.org/reference/items/generics.html#generic-parameters) explains the direct-record limitation. Diagnostic attributes are compiler hints, not a substitute for removing repeated failing constraints.

### Production boundary

The constructor model and terse grammar are now the preferred design, not a shipped API. Production must generate finite mappings directly by schema and relation identity and replace private runtime-helper coupling with a supported selection interface.

Shared nested DTO descendant handles, relative-path diagnostics, and view capabilities still need explicit interface decisions. Renamed dependencies require `runtime = path;`. Local record types need named DTO field moves at stable public return boundaries.

No editor completion, compile-time benchmark, or allocation benchmark is claimed. The next useful action is to define the supported field/selection contract from the checked prototype before changing production macros.

## Production release, 0.9.0

The constructor-checked design now ships in production. These crates are published on crates.io:

- [`rp-supabase-client` 0.9.0](https://crates.io/crates/rp-supabase-client/0.9.0).
- [`rp-supabase-codegen` 0.9.0](https://crates.io/crates/rp-supabase-codegen/0.9.0).
- [`rp-supabase-client-macros` 0.9.0](https://crates.io/crates/rp-supabase-client-macros/0.9.0).

The generator emits finite column and relationship mappings directly for each schema-qualified row type. Keys contain the complete normalized Rust identifier as const characters. No generated-source AST parsing, global key registry, or prototype dependency remains.

`select!` emits function-local owned records and copyable selection handles. Child relation types come from FK targets. Inline, shared DTO, inner, and predicate-only selections use the same supported runtime interface.

Shared DTO descendant handles compose with `.then(Dto::child)`. Named public results use `schema::named::<Relation, Dto>()`. `Query::select` now takes a selection value. Regenerate bindings and follow the client README migration section when upgrading from 0.8.

Independent review finds collisions between caller aliases and generated item or generic names. The corrected emitter chooses a deterministic prefix absent from every identifier in the invocation. It preserves caller spans and qualifies primitive types. Separate compiler controls cover root, DTO, runtime, outer generic, `str`, and `usize` aliases.

### Verification

- Workspace all-feature tests: 139 pass across 36 suites.
- Workspace doctests: nine pass.
- Workspace all-feature, all-target Clippy passes with warnings denied. Formatting passes.
- The offline executable prints the exact arbitrary-precision numeric value and nested relationship selection.
- All three packaged archives compile with Rust 1.85.1.
- A separate registry-only consumer compiles with Rust 1.85.1. It renames client and codegen dependencies and disables client default features.
- That consumer obtains fresh PostgreSQL metadata through the published generator and sends signed requests to PostgreSQL 17 and PostgREST 16.2.

The registry-only run preserves tenant roots `[7, 8]` and `[9]` in the original fixture. Its protected fixture preserves `[10, 11, 12]` and `[20, 21]`. Hidden non-null FK targets decode as `None`; hidden child collections are empty. Filtered, existence, inner, and predicate-only queries pass. Invoker and owner views retain their distinct policy behavior. Missing target privilege produces the expected rejection.

The same executable checks local and generic aliases, shared descendants, exact no-overfetch filter paths, raw aliases, and strict missing/duplicate decoding. Each live run removes its containers, anonymous volumes, and network.

All registry entries are non-yanked. Their SHA-256 checksums match the uploaded archives:

| Crate | SHA-256 |
| --- | --- |
| client 0.9.0 | `2dcfd71ad42ba56e25663c7588c6f502688ee766ae9f5d87c7fce0e8b9e6e11e` |
| codegen 0.9.0 | `5e3a8d76a2331f833a5b4bc9fb7efb3244eff8ae77fd82477c86ec22f8f4a1d2` |
| client-macros 0.9.0 | `81eac355429fcc27a6ccc0a888bd674e41dc8c48e41b09ae81271c8750096cf5` |

Direct same-schema base-table relationship capabilities remain unchanged. The release adds no inferred view, cross-schema, partition, self, or computed relationships. Static types still do not certify RLS isolation. No editor-completion, compile-time, or allocation benchmark is claimed.

### Release provenance correction

The initial 0.9.0 publication bypassed the existing release-plz workflow. All three archives record commit `21630f4` with `dirty: true`. Their implementation passes the checks above, but that commit does not contain the released changes.

Commit `84954d6` records the matching production source. The backfilled 0.9.0 tags disclose the original dirty publication. They do not rewrite its archive provenance.

`release-plz.toml` now requires a merged release PR before publication and groups the query-selection packages. The client declares Rust 1.85 support. The package manifests link their API documentation. The next release must come from a clean committed checkout through release-plz.

