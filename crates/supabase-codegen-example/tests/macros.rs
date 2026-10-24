rp_supabase_client::include_schema!("macro_bindings.rs");

#[test]
fn local_schema_macros_fill_the_runtime_for_selection_and_keys() {
    use public::tables::a_b;
    use rp_supabase_client::schema::{ColumnByKey, Selection};
    let selected = select!(a_b::Row => { id, body });
    let rows = selected.decode(r#"[{"id":7,"body":"typed"}]"#).unwrap();
    assert_eq!(rows[0].id, 7);
    assert_eq!(rows[0].body, "typed");
    let column = <a_b::Row as ColumnByKey<key!(type id)>>::COLUMN;
    let key = key!(id);
    let _: rp_supabase_client::schema::Key<key!(type id), _> = key;
    assert_eq!(rp_supabase_client::schema::params::eq(column, &7).1, "eq.7");
}
