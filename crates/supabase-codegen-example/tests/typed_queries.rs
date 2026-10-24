#[allow(dead_code)]
mod bindings {
    rp_supabase_client::include_schema!("contract_bindings.rs");
}
use bindings::public::tables::{a_b, typed_probe};
use rp_supabase_client::{rp_postgrest::Postgrest, schema::Field};
use serde_json::json;

rp_supabase_client::projection! {
    #[derive(Debug, PartialEq)]
    pub struct Probe for bindings::public::tables::typed_probe { id, display_name }
}
rp_supabase_client::projection! {
    struct Identity for bindings::public::tables::typed_probe { id }
}
rp_supabase_client::projection! {
    #[derive(Debug, PartialEq)]
    struct Labelled for bindings::public::tables::column_view { value, label }
}

#[test]
fn projection_requires_selected_fields_and_preserves_exact_json_names() {
    let probe: Probe =
        serde_json::from_str(r#"{"id":7,"display.name":null,"extra":true}"#).unwrap();
    assert_eq!(probe.id, 7);
    assert_eq!(probe.display_name, None);
    for invalid in [
        r#"{"id":7}"#,
        r#"{"id":7,"display_name":null}"#,
        r#"{"id":7,"display.name":false}"#,
        r#"{"id":7,"display.name":null,"display.name":"duplicate"}"#,
        r#"{"id":7,"id":8,"display.name":null}"#,
    ] {
        assert!(
            serde_json::from_str::<Probe>(invalid).is_err(),
            "accepted {invalid}"
        );
    }
}

#[tokio::test]
async fn inferred_projection_filter_and_mutation_smoke() {
    let mut server = mockito::Server::new_async().await;
    let selected = server
        .mock("GET", "/typed%2Eprobe")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "id,\"display.name\"".into()),
            mockito::Matcher::UrlEncoded("id".into(), "eq.7".into()),
            mockito::Matcher::UrlEncoded("\"display.name\"".into(), "is.null".into()),
        ]))
        .with_body(r#"[{"id":7,"display.name":null}]"#)
        .create_async()
        .await;
    let client = Postgrest::new(server.url()).unwrap();
    let rows = typed_probe::query(client.clone())
        .select(rp_supabase_client::schema::named::<_, Identity>())
        .select(rp_supabase_client::schema::named::<_, Probe>())
        .eq(typed_probe::columns::id, &7)
        .is_null(typed_probe::columns::display_name)
        .fetch()
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![Probe {
            id: 7,
            display_name: None
        }]
    );
    selected.assert_async().await;
    let inserted = server
        .mock("POST", "/typed%2Eprobe")
        .match_query(mockito::Matcher::UrlEncoded(
            "select".into(),
            "id,\"display.name\"".into(),
        ))
        .match_body(mockito::Matcher::Json(json!({"id":7})))
        .with_status(201)
        .with_body(r#"[{"id":7,"display.name":null}]"#)
        .create_async()
        .await;
    let rows = typed_probe::query(client.clone())
        .select(rp_supabase_client::schema::named::<_, Probe>())
        .insert(&typed_probe::Insert {
            id: 7,
            display_name: Field::Omit,
        })
        .fetch()
        .await
        .unwrap();
    assert_eq!(rows[0].id, 7);
    inserted.assert_async().await;
    let updated = server
        .mock("PATCH", "/typed%2Eprobe")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "id,\"display.name\"".into()),
            mockito::Matcher::UrlEncoded("id".into(), "eq.7".into()),
        ]))
        .match_body(mockito::Matcher::Json(json!({"display.name":null})))
        .with_body(r#"[{"id":7,"display.name":null}]"#)
        .create_async()
        .await;
    let rows = typed_probe::query(client.clone())
        .select(rp_supabase_client::schema::named::<_, Probe>())
        .update(&typed_probe::Update {
            display_name: Field::Value(None),
            ..Default::default()
        })
        .eq(typed_probe::columns::id, &7)
        .fetch()
        .await
        .unwrap();
    assert_eq!(rows[0].display_name, None);
    updated.assert_async().await;
    let deleted = server
        .mock("DELETE", "/typed%2Eprobe")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "id".into()),
            mockito::Matcher::UrlEncoded("id".into(), "eq.7".into()),
        ]))
        .with_body(r#"[{"id":7}]"#)
        .create_async()
        .await;
    let rows = typed_probe::query(client)
        .select(rp_supabase_client::schema::named::<_, Identity>())
        .delete()
        .eq(typed_probe::columns::id, &7)
        .fetch()
        .await
        .unwrap();
    assert_eq!(rows[0].id, 7);
    deleted.assert_async().await;
}

#[tokio::test]
async fn typed_fetch_rejects_http_errors_and_invalid_success_bodies() {
    let mut server = mockito::Server::new_async().await;
    for (status, body) in [
        (
            403,
            r#"{"code":"42501","message":"permission denied","details":null,"hint":null}"#,
        ),
        (502, "upstream unavailable"),
        (200, "not json"),
        (200, r#"[{"id":7}]"#),
        (200, r#"[{"id":"wrong","display.name":null}]"#),
    ] {
        let request = server
            .mock("GET", "/typed%2Eprobe")
            .match_query(mockito::Matcher::UrlEncoded(
                "select".into(),
                "id,\"display.name\"".into(),
            ))
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let error = typed_probe::query(Postgrest::new(server.url()).unwrap())
            .select(rp_supabase_client::schema::named::<_, Probe>())
            .fetch()
            .await
            .expect_err("invalid response must fail");
        match (status, error) {
            (403, rp_supabase_client::rp_postgrest::Error::Postgrest { metadata, .. }) => {
                assert_eq!(usize::from(metadata.status().as_u16()), status)
            }
            (502, rp_supabase_client::rp_postgrest::Error::Decode { metadata, .. }) => {
                assert_eq!(usize::from(metadata.status().as_u16()), status)
            }
            (
                200,
                rp_supabase_client::rp_postgrest::Error::ResponseDecode { source: error, .. },
            ) => {
                let expected = if body == "not json" {
                    serde_json::error::Category::Syntax
                } else {
                    serde_json::error::Category::Data
                };
                assert_eq!(error.classify(), expected);
            }
            (_, error) => panic!("wrong error variant for status {status}: {error:?}"),
        }
        request.assert_async().await;
    }
}

#[tokio::test]
async fn fetch_one_decodes_a_single_representation() {
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/a%23b")
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "*".into()))
        .match_header("accept", "application/vnd.pgrst.object+json")
        .with_body(r#"{"id":7,"body":"single","active":true}"#)
        .create_async()
        .await;
    let row = a_b::query(Postgrest::new(server.url()).unwrap())
        .fetch_one()
        .await
        .unwrap();
    assert_eq!(row.id, 7);
    request.assert_async().await;
}

#[tokio::test]
async fn reserved_text_filter_preserves_literal_value() {
    let mut server = mockito::Server::new_async().await;
    let value = "a,b.c:(d)*\"e\\f café";
    let request = server
        .mock("GET", "/typed%2Eprobe")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "id".into()),
            mockito::Matcher::UrlEncoded("\"display.name\"".into(), format!("eq.{value}")),
        ]))
        .with_body(r#"[{"id":7}]"#)
        .create_async()
        .await;
    let rows = typed_probe::query(Postgrest::new(server.url()).unwrap())
        .select(rp_supabase_client::schema::named::<_, Identity>())
        .eq(typed_probe::columns::display_name, value)
        .fetch()
        .await
        .unwrap();
    assert_eq!(rows[0].id, 7);
    request.assert_async().await;
}

#[tokio::test]
async fn computed_fields_are_selectable_filterable_and_orderable_but_not_default() {
    use bindings::{
        Outcome,
        public::tables::{column_view, json_probe},
    };
    use rp_supabase_client::{
        schema::{Order, Selection},
        select,
    };
    let mut server = mockito::Server::new_async().await;
    let client = Postgrest::new(server.url()).unwrap();
    let selected = server
        .mock("GET", "/json_probe")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "summary,outcome".into()),
            mockito::Matcher::UrlEncoded("summary".into(), "eq.hello".into()),
            mockito::Matcher::UrlEncoded("order".into(), "summary.desc".into()),
        ]))
        .with_body(r#"[{"summary":"hello","outcome":{"ok":true}},{"summary":null,"outcome":null}]"#)
        .create_async()
        .await;
    let rows = select!(json_probe::Row => { summary, outcome })
        .query(client.clone())
        .eq(json_probe::columns::summary, "hello")
        .order(json_probe::columns::summary, Order::Desc)
        .fetch()
        .await
        .unwrap();
    assert_eq!(rows[0].summary.as_deref(), Some("hello"));
    assert_eq!(rows[0].outcome, Some(Outcome { ok: true }));
    assert_eq!((&rows[1].summary, &rows[1].outcome), (&None, &None));
    selected.assert_async().await;

    let view = server
        .mock("GET", "/column_view")
        .match_query(mockito::Matcher::UrlEncoded(
            "select".into(),
            "value,label".into(),
        ))
        .with_body(r#"[{"value":1,"label":"one"}]"#)
        .create_async()
        .await;
    let rows = column_view::query(client.clone())
        .select(rp_supabase_client::schema::named::<_, Labelled>())
        .fetch()
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![Labelled {
            value: Some(1),
            label: Some("one".into())
        }]
    );
    view.assert_async().await;

    let default = server
        .mock("GET", "/column_view")
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "*".into()))
        .with_body(r#"[{"value":2}]"#)
        .create_async()
        .await;
    let rows = column_view::query(client).fetch().await.unwrap();
    assert_eq!(rows[0].value, Some(2));
    default.assert_async().await;
}

#[tokio::test]
async fn failed_payload_serialization_survives_projection_changes_without_sending() {
    use rp_supabase_client::schema::{Projection, Relation, WritableRelation, query};

    #[derive(serde::Deserialize)]
    struct FailingRow;
    impl Relation for FailingRow {
        type ColumnLookup = rp_supabase_client::schema::selection::RelationToken<Self>;
        const SCHEMA: &'static str = "public";
        const NAME: &'static str = "failing";
    }
    impl Projection<Self> for FailingRow {
        const SELECT_LEN: usize = 1;
        fn write_selection(output: &mut String) {
            output.push('*');
        }
        fn selection() -> std::borrow::Cow<'static, str> {
            std::borrow::Cow::Borrowed("*")
        }
    }
    struct FailingPayload;
    impl serde::Serialize for FailingPayload {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("cannot encode payload"))
        }
    }
    impl WritableRelation for FailingRow {
        type Insert = FailingPayload;
        type Update = FailingPayload;
    }

    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("POST", "/failing")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let error = query::<FailingRow>(Postgrest::new(server.url()).unwrap())
        .insert(&FailingPayload)
        .select(rp_supabase_client::schema::named::<_, FailingRow>())
        .fetch()
        .await
        .err()
        .unwrap();
    assert!(matches!(
        error,
        rp_supabase_client::rp_postgrest::Error::Serialization(_)
    ));
    request.assert_async().await;
}
