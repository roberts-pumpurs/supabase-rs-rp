#![expect(
    clippy::tests_outside_test_module,
    reason = "Integration tests exercise generated runtime contracts."
)]
#![expect(clippy::unwrap_used, reason = "Test setup retains failure details.")]

extern crate alloc;

use alloc::borrow::Cow;
use rp_supabase_client::{
    Error, Postgrest,
    schema::{Field, Function, Projection, Relation, WritableRelation, query, rpc},
};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Row {
    id: i32,
}
impl Relation for Row {
    type ColumnLookup = rp_supabase_client::schema::selection::RelationToken<Self>;
    const SCHEMA: &'static str = "public";
    const NAME: &'static str = "typed.probe";
}
impl Projection<Self> for Row {
    const SELECT_LEN: usize = 2;
    fn write_selection(output: &mut String) {
        output.push_str("id");
    }
    fn selection() -> Cow<'static, str> {
        Cow::Borrowed("id")
    }
}
impl WritableRelation for Row {
    type Insert = Field<String>;
    type Update = Field<String>;
}
struct Echo;
impl Function for Echo {
    type Args = Field<String>;
    type Returns = i32;
    const SCHEMA: &'static str = "private";
    const NAME: &'static str = "rpc?echo";
}

#[tokio::test]
async fn typed_and_raw_escape_preserve_deferred_serialization_failure() {
    let client = Postgrest::new("http://localhost:1").unwrap();
    assert!(matches!(
        query::<Row>(client.clone())
            .insert(&Field::Omit)
            .fetch()
            .await,
        Err(Error::Serialization(_))
    ));
    assert!(matches!(
        query::<Row>(client.clone())
            .update(&Field::Omit)
            .into_raw()
            .build(),
        Err(Error::Serialization(_))
    ));
    assert!(matches!(
        rpc::<Echo>(client.clone(), &Field::Omit).fetch().await,
        Err(Error::Serialization(_))
    ));
    assert!(matches!(
        rpc::<Echo>(client, &Field::Omit).into_raw().build(),
        Err(Error::Serialization(_))
    ));
}

#[tokio::test]
async fn typed_relation_and_rpc_use_literal_paths_and_canonical_executor() {
    let mut server = mockito::Server::new_async().await;
    let relation = server
        .mock("GET", "/typed%2Eprobe")
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "id".into()))
        .match_header("accept-profile", "public")
        .with_body(r#"[{"id":7}]"#)
        .create_async()
        .await;
    let function = server
        .mock("POST", "/rpc/rpc%3Fecho")
        .match_header("content-profile", "private")
        .match_body(r#""argument""#)
        .with_body("42")
        .create_async()
        .await;
    let client = Postgrest::new(server.url()).unwrap();
    let rows = query::<Row>(client.clone()).fetch().await.unwrap();
    assert_eq!(rows.as_slice(), [Row { id: 7_i32 }]);
    let result = rpc::<Echo>(client, &Field::Value("argument".into()))
        .fetch()
        .await
        .unwrap();
    assert_eq!(result, 42_i32);
    relation.assert_async().await;
    function.assert_async().await;
}

#[derive(Clone, Copy)]
struct Id;
impl rp_supabase_client::schema::Column for Id {
    type Relation = Row;
    type Value = i32;
    type Filter = i32;
    const NAME: &'static str = "id";
    const SELECT: &'static str = "id";
}
#[derive(Clone, Copy)]
struct Label;
impl rp_supabase_client::schema::Column for Label {
    type Relation = Row;
    type Value = String;
    type Filter = str;
    const NAME: &'static str = "label";
    const SELECT: &'static str = "label";
}
impl rp_supabase_client::schema::NullableColumn for Label {}
#[derive(Clone, Copy)]
struct Document;
impl rp_supabase_client::schema::Column for Document {
    type Relation = Row;
    type Value = serde_json::Value;
    type Filter = serde_json::Value;
    const NAME: &'static str = "document";
    const SELECT: &'static str = "document";
}
impl rp_supabase_client::schema::JsonColumn for Document {}

#[test]
fn pure_parameter_grammar_matches_typed_queries() {
    use rp_supabase_client::schema::{Nulls, Order, params};
    let values = ["a,b", "quote\"slash\\", " spaced "];
    let pairs = [
        params::eq(Id, &7_i32),
        params::in_(Label, values),
        params::is_null(Label),
        params::json_text_eq(Document, &["nested,key", "slug"], "a,b\"\\").unwrap(),
        params::order_with_nulls(Id, Order::Desc, Nulls::Last),
        params::projection::<Row, Row>(),
    ];
    assert_eq!(pairs[1].1, r#"in.("a,b","quote\"slash\\"," spaced ")"#);
    assert_eq!(params::neq(Id, &7_i32).1, "neq.7");
    assert_eq!(params::gt(Id, &7_i32).1, "gt.7");
    assert_eq!(params::gte(Id, &7_i32).1, "gte.7");
    assert_eq!(params::lt(Id, &7_i32).1, "lt.7");
    assert_eq!(params::lte(Id, &7_i32).1, "lte.7");
    assert_eq!(params::in_(Id, &[1_i32, 2_i32]).1, r#"in.("1","2")"#);
    let empty: &[i32] = &[];
    assert_eq!(params::in_(Id, empty).1, "in.()");
    assert_eq!(pairs[3].0, r#"document->"nested,key"->>slug"#);
    assert_eq!(pairs[3].1, "eq.a,b\"\\");
    let raw = query::<Row>(Postgrest::new("http://localhost:1").unwrap())
        .eq(Id, &7_i32)
        .in_(Label, values)
        .is_null(Label)
        .json_text_eq(Document, &["nested,key", "slug"], "a,b\"\\")
        .unwrap()
        .order_with_nulls(Id, Order::Desc, Nulls::Last)
        .into_raw();
    let actual: Vec<_> = raw.query_pairs().collect();
    let expected: Vec<_> = pairs
        .iter()
        .map(|(key, value)| (key.as_ref(), value.as_ref()))
        .collect();
    assert_eq!(actual, expected);
    assert!(matches!(
        params::json_text_eq(Document, &[], "value"),
        Err(Error::Configuration(
            rp_supabase_client::rp_postgrest::ConfigError::EmptyJsonPath
        ))
    ));
}

#[test]
fn typed_order_composes_and_pagination_keeps_selection() {
    use rp_supabase_client::schema::{Order, Paged, Query};
    let paged: Query<Row, Row, Paged> = query::<Row>(Postgrest::new("http://localhost:1").unwrap())
        .order(Id, Order::Asc)
        .order(Label, Order::Desc)
        .limit(0)
        .range(2, 4);
    let request = paged.into_raw().build().unwrap().build().unwrap();
    let pairs: Vec<_> = request.url().query_pairs().collect();
    assert_eq!(pairs.iter().filter(|(key, _)| key == "order").count(), 1);
    assert!(
        pairs
            .iter()
            .any(|(key, value)| key == "order" && value == "id.asc,label.desc")
    );
    assert!(
        pairs
            .iter()
            .any(|(key, value)| key == "limit" && value == "0")
    );
    assert!(
        pairs
            .iter()
            .any(|(key, value)| key == "select" && value == "id")
    );
    assert_eq!(request.headers()["range"], "2-4");
}

#[tokio::test]
async fn typed_count_and_minimal_mutations_use_owned_execution() {
    use rp_supabase_client::schema::Count;
    let mut server = mockito::Server::new_async().await;
    let get = server
        .mock("GET", "/typed%2Eprobe")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "id".into()),
            mockito::Matcher::UrlEncoded("limit".into(), "1".into()),
        ]))
        .match_header("prefer", "count=exact")
        .with_header("content-range", "0-0/9")
        .with_body(r#"[{"id":7}]"#)
        .create_async()
        .await;
    let head = server
        .mock("HEAD", "/typed%2Eprobe")
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "id".into()))
        .match_header("prefer", "count=planned")
        .with_header("content-range", "*/9")
        .create_async()
        .await;
    let delete = server
        .mock("DELETE", "/typed%2Eprobe")
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "id".into()))
        .match_header("prefer", "return=minimal")
        .with_status(204)
        .create_async()
        .await;
    let update = server
        .mock("PATCH", "/typed%2Eprobe")
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "id".into()))
        .match_header("prefer", mockito::Matcher::Regex("return=minimal".into()))
        .with_header("content-range", "*/3")
        .with_status(204)
        .create_async()
        .await;
    let client = Postgrest::new(server.url()).unwrap();
    let counted = query::<Row>(client.clone())
        .limit(1)
        .fetch_with_count(Count::Exact)
        .await
        .unwrap();
    assert_eq!(counted.data.as_slice(), [Row { id: 7_i32 }]);
    assert_eq!(counted.count, 9);
    assert_eq!(
        query::<Row>(client.clone())
            .count(Count::Planned)
            .await
            .unwrap(),
        9
    );
    query::<Row>(client.clone())
        .delete()
        .execute()
        .await
        .unwrap();
    assert_eq!(
        query::<Row>(client)
            .update(&Field::Value("updated".into()))
            .execute_with_count(Count::Exact)
            .await
            .unwrap(),
        3
    );
    get.assert_async().await;
    head.assert_async().await;
    delete.assert_async().await;
    update.assert_async().await;
}
