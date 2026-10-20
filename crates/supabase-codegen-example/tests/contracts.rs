#[allow(dead_code)]
mod bindings {
    rp_supabase_client::include_schema!("contract_bindings.rs");
}

use bindings::public::{functions, tables};

#[test]
fn column_contracts_agree_on_read_nullability_and_write_omission() {
    use rp_supabase_client::schema::{Column, Field, NullableColumn};
    use tables::{column_defaults as defaults, column_view as view};

    fn nullable_column<C: NullableColumn>(_: C) {}

    let row: defaults::Row = serde_json::from_str(
        r#"{"nullable":null,"defaulted":7,"by_default":8,"generated":9,"always":10}"#,
    )
    .unwrap();
    let nullable: <defaults::columns::nullable as Column>::Value = row.nullable;
    let defaulted: <defaults::columns::defaulted as Column>::Value = row.defaulted;
    let by_default: <defaults::columns::by_default as Column>::Value = row.by_default;
    let generated: <defaults::columns::generated as Column>::Value = row.generated;
    let always: <defaults::columns::always as Column>::Value = row.always;
    assert_eq!(nullable, None);
    assert_eq!((defaulted, by_default, generated, always), (7, 8, 9, 10));
    nullable_column(defaults::columns::nullable);

    assert_eq!(
        serde_json::to_value(defaults::Insert::default()).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        serde_json::to_value(defaults::Insert {
            nullable: Field::Value(None),
            defaulted: Field::Omit,
            by_default: Field::Value(11),
        })
        .unwrap(),
        serde_json::json!({"nullable": null, "by_default": 11})
    );
    assert_eq!(
        serde_json::to_value(defaults::Update {
            nullable: Field::Value(Some(12)),
            defaulted: Field::Value(13),
            by_default: Field::Omit,
        })
        .unwrap(),
        serde_json::json!({"nullable": 12, "defaulted": 13})
    );
    assert_eq!(
        serde_json::to_value(defaults::Update::default()).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        serde_json::to_value(tables::a_b::Insert {
            id: 1,
            body: "required".into(),
            active: Field::Omit,
        })
        .unwrap(),
        serde_json::json!({"id": 1, "body": "required"})
    );
    assert_eq!(
        serde_json::to_value(tables::a_b::Update::default()).unwrap(),
        serde_json::json!({})
    );

    let row: view::Row = serde_json::from_str(r#"{"value":null}"#).unwrap();
    let value: <view::columns::value as Column>::Value = row.value;
    assert_eq!(value, None);
    nullable_column(view::columns::value);
}

#[test]
fn generated_names_do_not_shadow_primitive_or_prelude_types() {
    let row: tables::a_b::Row =
        serde_json::from_str(r#"{"id":7,"body":"text alias","active":true}"#).unwrap();
    assert_eq!(row.body, "text alias");
    assert!(row.active);
    assert_eq!(serde_json::to_value(row).unwrap()["body"], "text alias");
}

#[test]
fn composite_and_output_rpc_results_match_json_cardinality() {
    let composite: functions::payload_rpc::Returns =
        serde_json::from_str(r#"{"label":"value","state":null}"#).unwrap();
    assert_eq!(composite.label.as_deref(), Some("value"));
    let table: functions::table_rpc::Returns =
        serde_json::from_str(r#"[{"value":"first","state":null},{"value":"second","state":null}]"#)
            .unwrap();
    assert_eq!(table[0].value.as_deref(), Some("first"));
    assert_eq!(table[1].value.as_deref(), Some("second"));
    let out: functions::single_out::Returns = serde_json::from_str(r#"{"plus":6}"#).unwrap();
    assert_eq!(out.plus, Some(6));
    let inout: functions::single_inout::Returns = serde_json::from_str(r#"{"num":9}"#).unwrap();
    assert_eq!(inout.num, Some(9));
    let named_record: functions::enum_record::Returns = serde_json::from_str(r#""ok""#).unwrap();
    assert!(matches!(
        named_record,
        Some(bindings::public::enums::Record::Ok)
    ));
}

#[tokio::test]
async fn quoted_relation_and_function_names_reach_the_correct_endpoint() {
    let mut server = mockito::Server::new_async().await;
    let relation = server
        .mock("GET", "/a%23b")
        .match_query(mockito::Matcher::UrlEncoded(
            "select".to_owned(),
            "*".to_owned(),
        ))
        .with_body(r#"[{"id":7,"body":"quoted","active":true}]"#)
        .create_async()
        .await;
    let client = rp_supabase_client::rp_postgrest::Postgrest::new(server.url()).unwrap();
    let rows = tables::a_b::query(client).fetch().await.unwrap();
    assert_eq!(rows[0].body, "quoted");
    relation.assert_async().await;
    let function = server
        .mock("POST", "/rpc/rpc%3Fecho")
        .with_body(r#""quoted RPC""#)
        .create_async()
        .await;
    let client = rp_supabase_client::rp_postgrest::Postgrest::new(server.url()).unwrap();
    let returned = rp_supabase_client::schema::rpc::<functions::rpc_echo::Function>(
        client,
        &functions::rpc_echo::Args {
            message: Some("input".to_owned()),
        },
    )
    .fetch()
    .await
    .unwrap();
    assert_eq!(returned.as_deref(), Some("quoted RPC"));
    function.assert_async().await;
}

#[tokio::test]
async fn typed_rpc_infers_composite_set_and_void_results_without_relation_semantics() {
    use rp_supabase_client::{
        rp_postgrest::Postgrest,
        schema::{Field, rpc},
    };
    let mut server = mockito::Server::new_async().await;
    let composite = server
        .mock("POST", "/rpc/payload_rpc")
        .match_query(mockito::Matcher::Missing)
        .match_header("content-profile", "public")
        .match_body(mockito::Matcher::Json(serde_json::json!({"label":"input"})))
        .with_body(r#"{"label":"returned","state":null}"#)
        .create_async()
        .await;
    let row = rpc::<functions::payload_rpc::Function>(
        Postgrest::new(server.url()).unwrap(),
        &functions::payload_rpc::Args {
            label: Some("input".into()),
        },
    )
    .fetch()
    .await
    .unwrap();
    assert_eq!(row.label.as_deref(), Some("returned"));
    assert!(row.state.is_none());
    composite.assert_async().await;

    for (label, expected_body) in [
        (Field::Omit, serde_json::json!({})),
        (Field::Value(None), serde_json::json!({"label":null})),
        (
            Field::Value(Some("literal".into())),
            serde_json::json!({"label":"literal"}),
        ),
    ] {
        let set = server
            .mock("POST", "/rpc/table_rpc")
            .match_query(mockito::Matcher::Missing)
            .match_body(mockito::Matcher::Json(expected_body))
            .with_body(r#"[{"value":"first","state":null},{"value":"second","state":null}]"#)
            .create_async()
            .await;
        let rows = rpc::<functions::table_rpc::Function>(
            Postgrest::new(server.url()).unwrap(),
            &functions::table_rpc::Args { label },
        )
        .fetch()
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value.as_deref(), Some("first"));
        assert_eq!(rows[1].value.as_deref(), Some("second"));
        set.assert_async().await;
    }

    let void = server
        .mock("POST", "/rpc/void_action")
        .match_query(mockito::Matcher::Missing)
        .match_body(mockito::Matcher::Json(serde_json::json!({})))
        .with_status(204)
        .create_async()
        .await;
    let () = rpc::<functions::void_action::Function>(
        Postgrest::new(server.url()).unwrap(),
        &functions::void_action::Args {},
    )
    .fetch()
    .await
    .unwrap();
    void.assert_async().await;
}

#[tokio::test]
async fn typed_rpc_reports_flat_errors_and_rejects_wrong_return_cardinality() {
    use rp_supabase_client::{
        rp_postgrest::{Error, Postgrest},
        schema::rpc,
    };
    let mut server = mockito::Server::new_async().await;
    for (status, body) in [
        (
            403,
            r#"{"code":"42501","message":"permission denied","details":null,"hint":null}"#,
        ),
        (502, "not an error envelope"),
        (200, r#"["wrong scalar cardinality"]"#),
        (200, "not JSON"),
    ] {
        let request = server
            .mock("POST", "/rpc/rpc%3Fecho")
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let error = rpc::<functions::rpc_echo::Function>(
            Postgrest::new(server.url()).unwrap(),
            &functions::rpc_echo::Args {
                message: Some("input".into()),
            },
        )
        .fetch()
        .await
        .unwrap_err();
        assert_eq!(error.status().unwrap().as_u16(), status as u16);
        match (status, error) {
            (403, Error::Postgrest { .. }) | (502, Error::Decode { .. }) => {}
            (200, Error::ResponseDecode { source, .. }) => {
                assert_eq!(
                    source.classify(),
                    if body == "not JSON" {
                        serde_json::error::Category::Syntax
                    } else {
                        serde_json::error::Category::Data
                    }
                );
            }
            (_, error) => panic!("unexpected RPC error: {error:?}"),
        }
        request.assert_async().await;
    }
    let raw = rpc::<functions::rpc_echo::Function>(
        Postgrest::new(server.url()).unwrap(),
        &functions::rpc_echo::Args { message: None },
    )
    .into_raw();
    let request = raw.build().unwrap().build().unwrap();
    assert_eq!(request.url().path(), "/rpc/rpc%3Fecho");
    assert!(request.url().query().is_none());
}

#[tokio::test]
async fn typed_rpc_rejects_wrong_composite_set_and_void_shapes() {
    use rp_supabase_client::{
        rp_postgrest::{Error, Postgrest},
        schema::{Field, rpc},
    };
    let mut server = mockito::Server::new_async().await;
    let composite = server
        .mock("POST", "/rpc/payload_rpc")
        .with_body(r#"[{"label":"not a single composite","state":null}]"#)
        .create_async()
        .await;
    let error = rpc::<functions::payload_rpc::Function>(
        Postgrest::new(server.url()).unwrap(),
        &functions::payload_rpc::Args { label: None },
    )
    .fetch()
    .await
    .err()
    .unwrap();
    assert!(matches!(error, Error::ResponseDecode { .. }));
    composite.assert_async().await;

    let set = server
        .mock("POST", "/rpc/table_rpc")
        .with_body(r#"{"value":"not a set","state":null}"#)
        .create_async()
        .await;
    let error = rpc::<functions::table_rpc::Function>(
        Postgrest::new(server.url()).unwrap(),
        &functions::table_rpc::Args { label: Field::Omit },
    )
    .fetch()
    .await
    .err()
    .unwrap();
    assert!(matches!(error, Error::ResponseDecode { .. }));
    set.assert_async().await;

    for body in ["", r#"{"not":"void"}"#] {
        let void = server
            .mock("POST", "/rpc/void_action")
            .with_body(body)
            .create_async()
            .await;
        let error = rpc::<functions::void_action::Function>(
            Postgrest::new(server.url()).unwrap(),
            &functions::void_action::Args {},
        )
        .fetch()
        .await
        .unwrap_err();
        assert!(matches!(error, Error::ResponseDecode { .. }));
        void.assert_async().await;
    }
}

#[test]
fn check_enum_and_external_override_share_write_and_filter_contracts() {
    use bindings::{
        AuthorKind,
        public::{enums::CheckProbeAuthorKind, tables::check_probe},
    };
    use rp_supabase_client::schema::{Column, Field, params};
    let row: check_probe::Row =
        serde_json::from_str(r#"{"author_kind":"organization","external_author":"user"}"#).unwrap();
    let owner: <check_probe::columns::author_kind as Column>::Filter = row.author_kind;
    let external: <check_probe::columns::external_author as Column>::Value = row.external_author;
    assert_eq!(owner.to_string(), "organization");
    assert_eq!(external, Some(AuthorKind::User));
    assert_eq!(
        params::eq(check_probe::columns::author_kind, &owner).1,
        "eq.organization"
    );
    let insert = check_probe::Insert {
        author_kind: CheckProbeAuthorKind::User,
        external_author: Field::Value(None),
    };
    assert_eq!(
        serde_json::to_value(insert).unwrap(),
        serde_json::json!({"author_kind":"user","external_author":null})
    );
    let update = check_probe::Update {
        author_kind: Field::Value(CheckProbeAuthorKind::Organization),
        external_author: Field::Value(Some(AuthorKind::User)),
    };
    assert_eq!(
        serde_json::to_value(update).unwrap(),
        serde_json::json!({"author_kind":"organization","external_author":"user"})
    );
}

#[test]
fn typed_json_preserves_sql_null_array_set_and_argument_contracts() {
    use bindings::{
        Outcome,
        public::{
            composites::JsonInfo,
            functions::{fetch_outcome, outcome_records},
            tables::json_probe,
        },
    };
    use rp_supabase_client::schema::{Array, Column, Field};
    let row: json_probe::Row = serde_json::from_str(
        r#"{"document":null,"documents":[{"ok":true},null],"extra":null,"notes":null}"#,
    )
    .unwrap();
    let document: <json_probe::columns::document as Column>::Value = row.document;
    assert_eq!(document, None);
    assert_eq!(
        row.documents,
        Array::Elements(vec![Some(Outcome { ok: true }), None])
    );
    let outcome: fetch_outcome::Returns = serde_json::from_str(r#"{"ok":true}"#).unwrap();
    assert_eq!(outcome, Some(Outcome { ok: true }));
    let records: outcome_records::Returns =
        serde_json::from_str(r#"[{"data":{"ok":true}}]"#).unwrap();
    assert!(records[0].data.ok);
    let composite: JsonInfo = serde_json::from_str(r#"{"data":{"ok":true}}"#).unwrap();
    assert!(composite.data.ok);
    assert_eq!(
        serde_json::to_value(fetch_outcome::Args {
            audience: Some(Outcome { ok: false })
        })
        .unwrap(),
        serde_json::json!({"audience":{"ok":false}})
    );
    let update = json_probe::Update {
        document: Field::Value(Some(Outcome { ok: true })),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(update).unwrap(),
        serde_json::json!({"document":{"ok":true}})
    );
    assert!(serde_json::from_str::<fetch_outcome::Returns>(r#"{"ok":"not a boolean"}"#).is_err());
}

#[test]
fn non_null_json_write_field_serializes_as_the_wrapped_value() {
    use rp_supabase_client::schema::{Array, Field, NonNullJson};
    use tables::json_probe;
    let extra = NonNullJson::try_from(serde_json::json!({"k": [1, null]})).unwrap();
    let insert = json_probe::Insert {
        document: Field::Omit,
        documents: Array::Elements(vec![]),
        extra: extra.clone(),
        notes: Field::Value(None),
    };
    assert_eq!(
        serde_json::to_value(insert).unwrap(),
        serde_json::json!({"documents":[],"extra":{"k":[1,null]},"notes":null})
    );
    let update = json_probe::Update {
        extra: Field::Value(extra),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(update).unwrap(),
        serde_json::json!({"extra":{"k":[1,null]}})
    );
}

#[test]
fn relationship_aliases_keep_marker_identity_and_wire_hints() {
    use bindings::public::tables::{customers, orders};
    use rp_supabase_client::{
        key,
        schema::{Relationship, RelationshipByKey},
    };
    fn same<T>(_: T, _: T) {}
    same(
        orders::relationships::orders_customer,
        orders::relationships::customer,
    );
    same(
        orders::relationships::orders_customer,
        orders::relationships::buyer,
    );
    same(
        customers::relationships::orders_orders_customer,
        customers::relationships::orders,
    );
    type CustomerEdge = <orders::Row as RelationshipByKey<key!(type customer)>>::Edge;
    type ReverseEdge = <customers::Row as RelationshipByKey<key!(type orders)>>::Edge;
    assert_eq!(CustomerEdge::HINT, "orders_customer");
    assert_eq!(ReverseEdge::HINT, "orders_customer");
    assert_eq!(CustomerEdge::RESOURCE, "customers");
    assert_eq!(ReverseEdge::RESOURCE, "orders");
}

#[test]
fn strict_inputs_preserve_nullable_opt_out_default_omission_and_custom_types() {
    use bindings::{Outcome, public::functions::strict_probe::Args};
    use rp_supabase_client::schema::Field;
    let args = Args {
        label: "required".into(),
        version: None,
        document: Field::Omit,
        payload: Outcome { ok: true },
    };
    assert_eq!(
        serde_json::to_value(args).unwrap(),
        serde_json::json!({"label":"required","version":null,"payload":{"ok":true}})
    );
    let args = Args {
        label: "required".into(),
        version: Some(3),
        document: Field::Value(None),
        payload: Outcome { ok: false },
    };
    assert_eq!(
        serde_json::to_value(args).unwrap(),
        serde_json::json!({"label":"required","version":3,"document":null,"payload":{"ok":false}})
    );
    let args = Args {
        label: "required".into(),
        version: None,
        document: Field::Value(Some(Outcome { ok: true })),
        payload: Outcome { ok: true },
    };
    assert_eq!(
        serde_json::to_value(args).unwrap()["document"],
        serde_json::json!({"ok":true})
    );
}
