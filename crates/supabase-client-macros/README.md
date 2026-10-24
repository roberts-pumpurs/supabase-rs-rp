# rp-supabase-client-macros

[![crates.io](https://img.shields.io/crates/v/rp-supabase-client-macros.svg)](https://crates.io/crates/rp-supabase-client-macros) [![docs.rs](https://docs.rs/rp-supabase-client-macros/badge.svg)](https://docs.rs/rp-supabase-client-macros)

Procedural macros for [`rp-supabase-client`](https://docs.rs/rp-supabase-client).
They build constructor-checked, query-local selections and lossless schema keys.

Do not depend on this crate directly. Use the re-exports `rp_supabase_client::select`
and `rp_supabase_client::key`. The generated code refers to `rp-supabase-client`
items, so the macros do not work without it. The macro version always matches
the `rp-supabase-client` version that re-exports it.

## Query-local records and diagnostics

Selected records expose ordinary public, movable fields. Their implementation-only
marker is private in a generated module, so unknown-field diagnostics list selected
fields rather than a synthetic marker. The module uses only its own value and marker
parameters: function-local aliases and generic roots remain supported.

Concrete generated relations resolve column names through table-local metadata,
including nested relationship targets and aliases of generated row types. This
provides the SQL table name and its actual valid column names. A generic root uses
the runtime lookup trait and retains readable unknown names, but cannot specialize
its diagnostic to a concrete table until its type is known.

`key!(name)` now produces `Key<K, N>`, where `K` is the lossless encoded identifier
and `N` is a block-local type named after the written identifier. Replace explicit
`Key<key!(type id)>` annotations with `Key<key!(type id), _>` at inference sites.
Keep generic bounds as `R: ColumnByKey<key!(type id)>`; `key!(type id)` is unchanged.
Do not try to name `N` outside the macro expression.

This uses stable Rust [method lookup](https://doc.rust-lang.org/reference/expressions/method-call-expr.html)
and [field privacy](https://doc.rust-lang.org/reference/visibility-and-privacy.html),
not identifier-path guessing or nightly diagnostic hooks.
