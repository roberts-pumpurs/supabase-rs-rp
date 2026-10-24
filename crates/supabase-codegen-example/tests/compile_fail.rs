//! Check real consumer snippets in an isolated Cargo project.
//! A positive control detects compiler and dependency setup failures.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn typed_contract_rejects_invalid_consumers() {
    let directory =
        std::env::temp_dir().join(format!("supabase-typed-consumers-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let client = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../supabase-client");
    let document = format!(
        r#"[package]
name = "supabase-typed-consumers"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
path = "consumer.rs"
[dependencies]
renamed = {{ package = "rp-supabase-client", path = {client:?} }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = {{ version = "1", features = ["arbitrary_precision"] }}
chrono = {{ version = "0.4", default-features = false, features = ["serde", "clock", "std"] }}
typed-builder = "0.23"
"#
    );
    fs::write(directory.join("Cargo.toml"), document).unwrap();
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
        directory.join("Cargo.lock"),
    )
    .unwrap();
    let header = format!(
        "#![allow(dead_code)]\nextern crate renamed as rp_supabase_client;\nmod bindings {{ include!({:?}); }}\nmod database {{ include!({:?}); }}\n#[allow(unused_imports, reason = \"Shared harness supports independent consumer snippets\")]\nuse bindings::public::tables;\nuse database::public;\nuse database::public::tables as rel;\nuse rp_supabase_client::rp_postgrest::Postgrest;\nuse rp_supabase_client::schema::Selection;\n#[path = {:?}] mod relationship_projections;\n#[allow(clippy::wildcard_imports, reason = \"Shared harness exposes the DTOs used by independent snippets\")]\nuse relationship_projections::*;\nrp_supabase_client::projection! {{ struct Id for bindings::public::tables::a_b {{ id }} }}\nrp_supabase_client::projection! {{ struct OtherOrder for database::public::tables::orders {{ id, billing: embed(database::public::tables::orders::relationships::orders_billing, AddressSummary) }} }}\n",
        PathBuf::from(env!("OUT_DIR")).join("contract_bindings.rs"),
        PathBuf::from(env!("OUT_DIR")).join("database.rs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/relationship_projections.rs"),
    );
    let renamed_runtimes = format!(
        r#"extern crate renamed as Name;
extern crate renamed as Selected;
extern crate renamed as __supabase_column_runtime;
mod runtime_name {{ include!({:?}); }}
mod runtime_selected {{ include!({:?}); }}
mod runtime_prefixed {{ include!({:?}); }}
fn consumer(client: Postgrest) {{
    let _ = rp_supabase_client::select!(runtime_name::public::tables::messages::Row => {{ id }}).query(client.clone());
    let _ = rp_supabase_client::select!(runtime_selected::public::tables::messages::Row => {{ id }}).query(client.clone());
    let _ = rp_supabase_client::select!(runtime_prefixed::public::tables::messages::Row => {{ id }}).query(client);
}}"#,
        PathBuf::from(env!("OUT_DIR")).join("runtime_name.rs"),
        PathBuf::from(env!("OUT_DIR")).join("runtime_selected.rs"),
        PathBuf::from(env!("OUT_DIR")).join("runtime_prefixed.rs"),
    );
    let cases = [
        (
            "codegen_runtime_generic_name_collision",
            renamed_runtimes.as_str(),
            true,
        ),
        (
            "key_runtime_name_equals_key",
            "fn consumer() { use renamed as id; let _=id::key!(runtime=id; id); }",
            true,
        ),
        (
            "runtime_name_equals_column",
            "fn consumer() { use renamed as id; let selected=id::select!(runtime=id; rel::orders::Row => { id }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "runtime_name_equals_relationship",
            "fn consumer() { use renamed as orders_billing; let selected=orders_billing::select!(runtime=orders_billing; rel::orders::Row => { billing: orders_billing { id } }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "readable_path_key_typo",
            "fn consumer() { let s=rp_supabase_client::select!(rel::orders::Row => { id, billing: embed(orders_billing,AddressSummary) }); let _=s.billing.then(AddressSummary::country).column(rp_supabase_client::key!(nmae)); }",
            false,
        ),
        (
            "readable_handle_key_typo",
            "fn consumer() { let s=rp_supabase_client::select!(rel::orders::Row => { id, billing: orders_billing { id } }); let _=s.billing.column(rp_supabase_client::key!(lable)); }",
            false,
        ),
        (
            "concrete_notes_valid_names",
            "fn consumer() { type Root=bindings::public::tables::notes::Row; let _=rp_supabase_client::select!(Root => { id, title }); }",
            false,
        ),
        (
            "movable_raw_unicode_fields",
            "fn consumer() { let s=rp_supabase_client::select!(bindings::shared::tables::typed_probe::Row => { id, r#type, café }); let mut rows=s.decode(\"[]\").unwrap(); let row=rows.remove(0); let _id=row.id; let _kind=row.r#type; let _name=row.café; }",
            true,
        ),
        (
            "generic_empty_only",
            "fn consumer<R>() where R: rp_supabase_client::schema::Relation + rp_supabase_client::schema::Projection<R> + rp_supabase_client::schema::RelationshipByKey<rp_supabase_client::key!(type orders_billing)> { let s=rp_supabase_client::select!(R => { matched: empty(orders_billing) }); let _=s.decode(\"[]\"); }",
            true,
        ),
        (
            "readable_key_typo",
            "fn consumer() { let s=rp_supabase_client::select!(rel::orders::Row => { id }); let _=s.column(rp_supabase_client::key!(titl)); }",
            false,
        ),
        (
            "private_record_marker",
            "fn consumer() { let s=rp_supabase_client::select!(rel::orders::Row => { id }); let rows=s.decode(\"[]\").unwrap(); let _=&rows[0].missing; }",
            false,
        ),
        (
            "named_projection_runtime_and_item_hygiene",
            "#[allow(non_camel_case_types)] fn consumer(c: Postgrest) { use renamed as __selection_Runtime; struct str; struct usize; type __selection_Alias0=AddressSummary; __selection_Runtime::projection! { #[derive(Debug)] pub struct __selection_KeyVisitor for rel::orders { id, r#select: embed(rel::orders::relationships::orders_billing, __selection_Alias0, inner), matched: empty(rel::orders::relationships::order_details_details_order) } } let _: __selection_KeyVisitor=serde_json::from_str(\"{\\\"id\\\":7,\\\"select\\\":null}\").unwrap(); let _=rel::orders::query(c).select(__selection_Runtime::schema::named::<_, __selection_KeyVisitor>()).embedded(__selection_KeyVisitor::r#select.then(AddressSummary::country), |_| {}).exists(__selection_KeyVisitor::matched); }",
            true,
        ),
        (
            "root_private_item_alias_hygiene",
            "fn consumer() { use rel::orders::Row as __Record; let selected=rp_supabase_client::select!(__Record => { id }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "dto_alias_item_hygiene",
            "fn consumer() { type __Alias0=AddressSummary; let selected=rp_supabase_client::select!(rel::orders::Row => { billing: embed(orders_billing,__Alias0) }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "dto_descriptor_item_hygiene",
            "fn consumer() { type __Descriptor=AddressSummary; let selected=rp_supabase_client::select!(rel::orders::Row => { billing: embed(orders_billing,__Descriptor) }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "dto_visitor_item_hygiene",
            "fn consumer() { type __Visitor=AddressSummary; let selected=rp_supabase_client::select!(rel::orders::Row => { billing: embed(orders_billing,__Visitor) }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "dto_key_visitor_item_hygiene",
            "fn consumer() { type __KeyVisitor=AddressSummary; let selected=rp_supabase_client::select!(rel::orders::Row => { billing: embed(orders_billing,__KeyVisitor) }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "runtime_generic_name_alias_hygiene",
            "fn consumer() { use rp_supabase_client as __R; let selected=__R::select!(runtime=__R; rel::orders::Row => { id }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "runtime_private_item_alias_hygiene",
            "fn consumer() { use rp_supabase_client as __Descriptor; let selected=__Descriptor::select!(runtime=__Descriptor; rel::orders::Row => { id }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "bounded_outer_generic_name_hygiene",
            "fn consumer<__R>() where __R: rp_supabase_client::schema::Relation + rp_supabase_client::schema::Projection<__R> + rp_supabase_client::schema::ColumnByKey<rp_supabase_client::key!(type id)> { let selected=rp_supabase_client::select!(__R => { id }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "primitive_str_hygiene",
            "#[allow(non_camel_case_types)] fn consumer() { struct str; let selected=rp_supabase_client::select!(rel::orders::Row => { id, billing: orders_billing { label } }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "primitive_usize_hygiene",
            "#[allow(non_camel_case_types)] fn consumer() { struct usize; let selected=rp_supabase_client::select!(rel::orders::Row => { id, billing: orders_billing { label } }); let _=selected.decode(\"[]\"); }",
            true,
        ),
        (
            "local_selection_positive",
            "fn consumer(c: Postgrest) { use rel::orders::Row as Root; type LocalDto = AddressSummary; use rp_supabase_client::{select,key}; let s=select!(Root => { id, billing: embed(orders_billing,LocalDto), shipping: orders_shipping { label }, matched: empty(order_details_details_order,inner) }); let _=s.query(c).embedded(s.billing.then(AddressSummary::country), |country| { country.eq(rel::countries::columns::name,\"France\"); }).embedded(s.billing, |address| { address.embedded(AddressSummary::country, |_| {}); }).embedded(s.shipping, |address| { address.eq(rel::addresses::columns::id,&7); }).exists(s.matched).delete().eq(s.column(key!(id)),&7); }",
            true,
        ),
        (
            "generic_root_and_dto",
            "fn consumer<R,P>(c: Postgrest) where R: rp_supabase_client::schema::Relation + rp_supabase_client::schema::Projection<R> + rp_supabase_client::schema::ColumnByKey<rp_supabase_client::key!(type id)> + rp_supabase_client::schema::RelationshipByKey<rp_supabase_client::key!(type orders_billing)>, P: rp_supabase_client::schema::Projection<<<R as rp_supabase_client::schema::RelationshipByKey<rp_supabase_client::key!(type orders_billing)>>::Edge as rp_supabase_client::schema::Relationship>::Target> { let s=rp_supabase_client::select!(R => { id, billing: embed(orders_billing,P) }); let _=s.query(c); }",
            true,
        ),
        (
            "runtime_alias_hygiene",
            "fn consumer(c: Postgrest) { use rp_supabase_client as renamed; use rel::orders::Row as Root; struct Option; struct Result; struct String; struct Vec; let s=renamed::select!(runtime = renamed; Root => { id, r#type: orders_billing { label }, café: orders_shipping { id } }); let _=s.column(renamed::key!(runtime = renamed; id)); let _=s.query(c); }",
            true,
        ),
        (
            "same_table_schema_identity",
            "fn consumer(c: Postgrest) { let a=rp_supabase_client::select!(bindings::public::tables::typed_probe::Row => { id }); let b=rp_supabase_client::select!(bindings::shared::tables::typed_probe::Row => { id, r#type, café }); let _=a.query(c.clone()).eq(a.column(rp_supabase_client::key!(id)),&7); let _=b.query(c).eq(b.column(rp_supabase_client::key!(id)),\"text\"); }",
            true,
        ),
        (
            "inline_nominal_owner",
            "fn consumer(c: Postgrest) { let a=rp_supabase_client::select!(rel::orders::Row => { id, billing: orders_billing { id } }); let b=rp_supabase_client::select!(rel::orders::Row => { id, billing: orders_billing { id } }); let _=a.query(c).embedded(b.billing, |_| {}); }",
            false,
        ),
        (
            "inline_wrong_target",
            "fn consumer(c: Postgrest) { let _=rp_supabase_client::select!(rel::orders::Row => { id, billing: embed(orders_billing,CountrySummary) }).query(c); }",
            false,
        ),
        (
            "inline_wrong_source",
            "fn consumer(c: Postgrest) { let _=rp_supabase_client::select!(rel::customers::Row => { id, billing: orders_billing { id } }).query(c); }",
            false,
        ),
        (
            "inline_locked_reselect",
            "fn consumer(c: Postgrest) { let s=rp_supabase_client::select!(rel::orders::Row => { id, billing: orders_billing { id } }); let _=s.query(c).embedded(s.billing, |_| {}).select(s); }",
            false,
        ),
        (
            "inline_empty_no_decoded_field",
            "fn consumer() { let s=rp_supabase_client::select!(rel::orders::Row => { id, matched: empty(order_details_details_order) }); let rows=s.decode(\"[]\").unwrap(); let _=&rows[0].matched; }",
            false,
        ),
        (
            "inline_empty_no_descendant",
            "fn consumer(c: Postgrest) { let s=rp_supabase_client::select!(rel::orders::Row => { id, matched: empty(orders_billing) }); let _=s.query(c).embedded(s.matched.then(AddressSummary::country), |_| {}); }",
            false,
        ),
        (
            "inline_unselected_column",
            "fn consumer() { let s=rp_supabase_client::select!(rel::orders::Row => { id }); let rows=s.decode(\"[]\").unwrap(); let _=&rows[0].label; }",
            false,
        ),
        (
            "schema_key_wrong_value",
            "fn consumer(c: Postgrest) { let s=rp_supabase_client::select!(bindings::shared::tables::typed_probe::Row => { id }); let _=s.query(c).eq(s.column(rp_supabase_client::key!(id)),&7); }",
            false,
        ),
        (
            "shared_content",
            "rp_supabase_client::projection! { struct Content for [database::public::tables::articles, database::public::tables::videos] { id, name, owner_id } } fn consumer(c: Postgrest) { let _ = rel::articles::query(c.clone()).select(rp_supabase_client::schema::named::<_, Content>()); let _ = rel::videos::query(c).select(rp_supabase_client::schema::named::<_, Content>()); }",
            true,
        ),
        (
            "shared_unselected_columns_need_not_match",
            "rp_supabase_client::projection! { struct SharedId for [database::public::tables::articles, database::public::tables::content_wrong_type, database::public::tables::content_missing] { id } } fn consumer(c: Postgrest) { let _ = rel::articles::query(c.clone()).select(rp_supabase_client::schema::named::<_, SharedId>()); let _ = rel::content_wrong_type::query(c.clone()).select(rp_supabase_client::schema::named::<_, SharedId>()); let _ = rel::content_missing::query(c).select(rp_supabase_client::schema::named::<_, SharedId>()); }",
            true,
        ),
        (
            "shared_type_mismatch",
            "rp_supabase_client::projection! { struct Invalid for [database::public::tables::articles, database::public::tables::content_wrong_type] { id, owner_id } }",
            false,
        ),
        (
            "shared_key_mismatch",
            "rp_supabase_client::projection! { struct Invalid for [database::public::tables::articles, database::public::tables::content_wrong_key] { id, owner_id } }",
            false,
        ),
        (
            "shared_missing_field",
            "rp_supabase_client::projection! { struct Invalid for [database::public::tables::articles, database::public::tables::content_missing] { id, owner_id } }",
            false,
        ),
        (
            "shared_missing_binding",
            "rp_supabase_client::projection! { struct Content for [database::public::tables::articles, database::public::tables::videos] { id, name, owner_id } } fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, Content>()); }",
            false,
        ),
        (
            "explicit_embed_source",
            "impl rp_supabase_client::schema::Projection<rel::customers::Row> for OrderSummary { const SELECT_LEN: usize = 2; fn write_selection(s: &mut String) { s.push_str(\"id\"); } } fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, OrderSummary>()).embedded(OrderSummary::billing, |_| {}); }",
            false,
        ),
        (
            "valid_typed_read_operations",
            "fn consumer(c: Postgrest) { use rp_supabase_client::schema::Order; let _ = rel::articles::query(c).order(rel::articles::columns::id, Order::Desc).in_(rel::articles::columns::id, [&1i64, &2]).json_text_eq(rel::articles::columns::document, &[\"slug\"], \"value\").unwrap().limit(2).range(0, 1); }",
            true,
        ),
        (
            "paged_delete",
            "fn consumer(c: Postgrest) { let _ = rel::articles::query(c).limit(1).delete(); }",
            false,
        ),
        (
            "ranged_update",
            "fn consumer(c: Postgrest, p: rel::articles::Update) { let _ = rel::articles::query(c).range(0, 1).update(&p); }",
            false,
        ),
        (
            "paged_insert",
            "fn consumer(c: Postgrest, p: rel::articles::Insert) { let _ = rel::articles::query(c).limit(1).insert(&p); }",
            false,
        ),
        (
            "wrong_order_owner",
            "fn consumer(c: Postgrest) { let _ = rel::articles::query(c).order(rel::videos::columns::id, rp_supabase_client::schema::Order::Asc); }",
            false,
        ),
        (
            "wrong_in_owner",
            "fn consumer(c: Postgrest) { let _ = rel::articles::query(c).in_(rel::videos::columns::id, [&1i64]); }",
            false,
        ),
        (
            "wrong_in_value",
            "fn consumer(c: Postgrest) { let _ = rel::articles::query(c).in_(rel::articles::columns::id, [\"one\"]); }",
            false,
        ),
        (
            "wrong_json_owner",
            "fn consumer(c: Postgrest) { let _ = rel::articles::query(c).json_text_eq(rel::videos::columns::document, &[\"slug\"], \"value\"); }",
            false,
        ),
        (
            "non_json_path",
            "fn consumer(c: Postgrest) { let _ = rel::articles::query(c).json_text_eq(rel::articles::columns::name, &[\"slug\"], \"value\"); }",
            false,
        ),
        (
            "valid",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).select(rp_supabase_client::schema::named::<_, Id>()).eq(tables::a_b::columns::id, &7); }",
            true,
        ),
        (
            "unknown_column",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).eq(tables::a_b::columns::missing, &7); }",
            false,
        ),
        (
            "wrong_relation_column",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).eq(tables::bool::columns::value, &true); }",
            false,
        ),
        (
            "wrong_value",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).eq(tables::a_b::columns::id, &\"seven\"); }",
            false,
        ),
        (
            "wrong_projection",
            "fn consumer(c: Postgrest) { let _ = tables::bool::query(c).select(rp_supabase_client::schema::named::<_, Id>()); }",
            false,
        ),
        (
            "wrong_payload",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).insert(&tables::bool::Insert { value: true }); }",
            false,
        ),
        (
            "raw_payload",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).update(&serde_json::json!({\"id\":7})); }",
            false,
        ),
        (
            "unselected_field",
            "fn consumer(row: Id) { let _ = row.body; }",
            false,
        ),
        (
            "nonnull_null",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).is_null(tables::a_b::columns::id); }",
            false,
        ),
        (
            "nullable_value",
            "fn consumer(c: Postgrest) { let _ = tables::typed_probe::query(c).eq(tables::typed_probe::columns::display_name, &Some(String::from(\"name\"))); }",
            false,
        ),
        (
            "invalid_projection_column",
            "rp_supabase_client::projection! { struct Invalid for bindings::public::tables::a_b { missing } }",
            false,
        ),
        (
            "view_write",
            "fn consumer(c: Postgrest) { let _ = database::public::tables::message_summaries::query(c).delete(); }",
            false,
        ),
        (
            "second_mutation",
            "fn consumer(c: Postgrest) { let _ = tables::a_b::query(c).delete().delete(); }",
            false,
        ),
        (
            "valid_nested_locked_write",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerSummary>()).embedded(CustomerSummary::orders.then(OrderSummary::billing).then(AddressSummary::country), |child| { child.eq(rel::countries::columns::name, \"literal\"); }).exists(CustomerSummary::orders).delete().eq(rel::customers::columns::id, &7); }",
            true,
        ),
        (
            "valid_empty_scoped_and_write_first",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerPredicates>()).delete().embedded(CustomerPredicates::matching_orders, |child| { child.eq(rel::orders::columns::id, &7); }).not_exists(CustomerPredicates::matching_orders); }",
            true,
        ),
        (
            "wrong_embed_source",
            "rp_supabase_client::projection! { struct WrongSource for database::public::tables::customers { id, address: embed(database::public::tables::orders::relationships::orders_billing, AddressSummary) } }",
            false,
        ),
        (
            "wrong_embed_target",
            "rp_supabase_client::projection! { struct WrongTarget for database::public::tables::orders { id, address: embed(database::public::tables::orders::relationships::orders_billing, CountrySummary) } }",
            false,
        ),
        (
            "wrong_root_handle",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerSummary>()).embedded(OrderSummary::billing, |_| {}); }",
            false,
        ),
        (
            "unselected_child_projection_handle",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerSummary>()).embedded(CustomerSummary::orders.then(OtherOrder::billing), |_| {}); }",
            false,
        ),
        (
            "unselected_handle_at_root",
            "fn consumer(c: Postgrest) { let _ = rel::orders::query(c).embedded(OrderSummary::billing, |_| {}); }",
            false,
        ),
        (
            "wrong_scoped_column",
            "fn consumer(c: Postgrest) { let _ = rel::orders::query(c).select(rp_supabase_client::schema::named::<_, OrderSummary>()).embedded(OrderSummary::billing, |child| { child.eq(rel::countries::columns::id, &7); }); }",
            false,
        ),
        (
            "wrong_scoped_value",
            "fn consumer(c: Postgrest) { let _ = rel::orders::query(c).select(rp_supabase_client::schema::named::<_, OrderSummary>()).embedded(OrderSummary::billing, |child| { child.eq(rel::addresses::columns::id, \"seven\"); }); }",
            false,
        ),
        (
            "select_after_embedded",
            "fn consumer(c: Postgrest) { let _ = rel::orders::query(c).select(rp_supabase_client::schema::named::<_, OrderSummary>()).embedded(OrderSummary::billing, |_| {}).select(rp_supabase_client::schema::named::<_, OrderInner>()); }",
            false,
        ),
        (
            "select_after_exists",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerPredicates>()).exists(CustomerPredicates::matching_orders).select(rp_supabase_client::schema::named::<_, CustomerSummary>()); }",
            false,
        ),
        (
            "select_after_not_exists",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerPredicates>()).not_exists(CustomerPredicates::matching_orders).select(rp_supabase_client::schema::named::<_, CustomerSummary>()); }",
            false,
        ),
        (
            "select_after_locked_write",
            "fn consumer(c: Postgrest) { let _ = rel::orders::query(c).select(rp_supabase_client::schema::named::<_, OrderSummary>()).embedded(OrderSummary::billing, |_| {}).delete().select(rp_supabase_client::schema::named::<_, OrderInner>()); }",
            false,
        ),
        (
            "wrong_exists_handle",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerPredicates>()).exists(OrderSummary::billing); }",
            false,
        ),
        (
            "empty_has_no_decoded_field",
            "fn consumer(row: CustomerPredicates) { let _ = row.matching_orders; }",
            false,
        ),
        (
            "empty_has_no_selected_child_handle",
            "fn consumer(c: Postgrest) { let _ = rel::customers::query(c).select(rp_supabase_client::schema::named::<_, CustomerPredicates>()).embedded(CustomerPredicates::matching_orders.then(OrderSummary::billing), |_| {}); }",
            false,
        ),
        (
            "root_rejects_child_column",
            "fn consumer(c: Postgrest) { let _ = rel::orders::query(c).select(rp_supabase_client::schema::named::<_, OrderSummary>()).eq(rel::addresses::columns::id, &7); }",
            false,
        ),
        (
            "duplicate_scalar_embed_alias",
            "rp_supabase_client::projection! { struct Duplicate for database::public::tables::orders { id, id: embed(database::public::tables::orders::relationships::orders_billing, AddressSummary) } }",
            false,
        ),
        (
            "duplicate_embed_alias",
            "rp_supabase_client::projection! { struct Duplicate for database::public::tables::orders { billing: embed(database::public::tables::orders::relationships::orders_billing, AddressSummary), billing: empty(database::public::tables::orders::relationships::orders_shipping) } }",
            false,
        ),
        (
            "to_one_is_not_unconditional",
            "fn consumer(row: OrderInner) { let _: AddressSummary = row.billing; }",
            false,
        ),
        (
            "to_many_is_not_optional",
            "fn consumer(row: CustomerSummary) { let _: Option<OrderSummary> = row.orders; }",
            false,
        ),
        (
            "valid_inferred_rpc",
            "async fn consumer(c: Postgrest) -> Result<(), rp_supabase_client::rp_postgrest::Error> { let value = rp_supabase_client::schema::rpc::<bindings::public::functions::rpc_echo::Function>(c, &bindings::public::functions::rpc_echo::Args { message: None }).fetch().await?; let _: Option<String> = value; Ok(()) }",
            true,
        ),
        (
            "valid_rpc_raw_escape",
            "fn consumer(c: Postgrest) { let _: rp_supabase_client::rp_postgrest::Builder = rp_supabase_client::schema::rpc::<bindings::public::functions::rpc_echo::Function>(c, &bindings::public::functions::rpc_echo::Args { message: None }).into_raw(); }",
            true,
        ),
        (
            "rpc_wrong_args",
            "fn consumer(c: Postgrest) { let _ = rp_supabase_client::schema::rpc::<bindings::public::functions::rpc_echo::Function>(c, &bindings::public::functions::payload_rpc::Args { label: None }); }",
            false,
        ),
        (
            "rpc_cannot_override_return",
            "async fn consumer(c: Postgrest) { let _ = rp_supabase_client::schema::rpc::<bindings::public::functions::rpc_echo::Function>(c, &bindings::public::functions::rpc_echo::Args { message: None }).fetch::<Vec<String>>().await; }",
            false,
        ),
        (
            "rpc_has_no_relation_projection",
            "fn consumer(c: Postgrest) { let _ = rp_supabase_client::schema::rpc::<bindings::public::functions::rpc_echo::Function>(c, &bindings::public::functions::rpc_echo::Args { message: None }).select(rp_supabase_client::schema::named::<_, Id>()); }",
            false,
        ),
        (
            "shared_filter_mismatched_types",
            "rp_supabase_client::projection! { struct Content for [rel::articles, rel::videos] { name } filters { content_id: [id, name] } }",
            false,
        ),
        (
            "shared_filter_wrong_query_relation",
            "rp_supabase_client::projection! { struct Content for [rel::articles, rel::videos] { name } filters { content_id: [id, owner_id] } } fn reject(client: Postgrest) { rel::videos::query(client).eq(Content::content_id::<rel::articles::Row>(), &7); }",
            false,
        ),
        (
            "shared_filter_undeclared_relation",
            "rp_supabase_client::projection! { struct Content for [rel::articles, rel::videos] { name } filters { content_id: [id, owner_id] } } fn reject() { let _ = Content::content_id::<rel::content_links::Row>(); }",
            false,
        ),
        (
            "shared_filter_missing_mapping",
            "rp_supabase_client::projection! { struct Content for [rel::articles, rel::videos] { name } filters { content_id: [id] } }",
            false,
        ),
        (
            "computed_field_filters_and_selects",
            "fn consumer(c: Postgrest, v: serde_json::Value) { let _ = tables::json_probe::query(c.clone()).is_null(tables::json_probe::columns::summary).is_null(tables::json_probe::columns::outcome); let _ = rp_supabase_client::select!(tables::column_view::Row => { value, label }).query(c); let _ = tables::json_probe::Insert { document: rp_supabase_client::schema::Field::Omit, documents: rp_supabase_client::schema::Array::Elements(vec![]), extra: rp_supabase_client::schema::NonNullJson::from(serde_json::Map::new()), notes: rp_supabase_client::schema::Field::Value(Some(v)) }; }",
            true,
        ),
        (
            "computed_field_not_in_row",
            "fn consumer(row: tables::json_probe::Row) { let _ = row.summary; }",
            false,
        ),
        (
            "computed_field_not_in_insert",
            "fn consumer(v: rp_supabase_client::schema::NonNullJson) { let _ = tables::json_probe::Insert { document: rp_supabase_client::schema::Field::Omit, documents: rp_supabase_client::schema::Array::Elements(vec![]), extra: v, notes: rp_supabase_client::schema::Field::Omit, summary: None }; }",
            false,
        ),
        (
            "computed_field_not_in_update",
            "fn consumer() { let _ = tables::json_probe::Update { summary: rp_supabase_client::schema::Field::Omit, ..Default::default() }; }",
            false,
        ),
        (
            "non_null_json_rejects_raw_value",
            "fn consumer(v: serde_json::Value) { let _ = tables::json_probe::Insert { document: rp_supabase_client::schema::Field::Omit, documents: rp_supabase_client::schema::Array::Elements(vec![]), extra: v, notes: rp_supabase_client::schema::Field::Omit }; }",
            false,
        ),
    ];
    for (name, source, succeeds) in cases {
        fs::write(
            directory.join("consumer.rs"),
            format!("{header}\n{source}\n"),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO"));
        command.current_dir(&directory);
        if name == "local_selection_positive" {
            command.args([
                "clippy",
                "--offline",
                "--quiet",
                "--",
                "-D",
                "warnings",
                "-W",
                "clippy::pedantic",
                "-W",
                "clippy::nursery",
            ]);
        } else {
            command.args(["check", "--offline", "--quiet"]);
        }
        if let Some(toolchain) = option_env!("RUSTUP_TOOLCHAIN") {
            command.env("RUSTUP_TOOLCHAIN", toolchain);
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.success(),
            succeeds,
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        if matches!(
            name,
            "readable_key_typo" | "readable_path_key_typo" | "readable_handle_key_typo"
        ) {
            let typo = match name {
                "readable_path_key_typo" => "nmae",
                "readable_handle_key_typo" => "lable",
                _ => "titl",
            };
            assert!(diagnostics.contains(typo), "{diagnostics}");
            assert!(!diagnostics.contains("Character<"), "{diagnostics}");
            assert_eq!(diagnostics.matches("error[E").count(), 1, "{diagnostics}");
        }
        if name == "private_record_marker" {
            assert!(!diagnostics.contains("__selection_marker"), "{diagnostics}");
        }
        if name == "concrete_notes_valid_names" {
            for expected in ["title", "public.notes", "id", "owner", "owner_name", "body"] {
                assert!(diagnostics.contains(expected), "{diagnostics}");
            }
            assert!(!diagnostics.contains("Character<"), "{diagnostics}");
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
