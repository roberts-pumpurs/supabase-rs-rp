#[allow(dead_code)]
mod database {
    rp_supabase_client::include_schema!("database.rs");
}
use database::public::tables::{articles, videos};
use rp_supabase_client::{
    rp_postgrest::{Count, Postgrest},
    schema::{Nulls, Order, Projection, params},
};

rp_supabase_client::projection! {
    #[derive(Debug, PartialEq)]
    struct Content for [database::public::tables::articles, database::public::tables::videos] { id, name, owner_id }
}
rp_supabase_client::projection! {
    struct Linked for database::public::tables::content_links {
        id,
        article: embed(database::public::tables::content_links::relationships::links_article, Content),
        video: embed(database::public::tables::content_links::relationships::links_video, Content),
    }
}

#[test]
fn shared_child_retains_each_relationship_target_and_handle() {
    use database::public::tables::content_links;
    let row: Linked = serde_json::from_str(r#"{"id":4,"article":{"id":1,"name":"s","owner_id":9},"video":{"id":2,"name":"a","owner_id":9}}"#).unwrap();
    assert_eq!(row.article.as_ref().unwrap().id, 1);
    assert_eq!(row.video.as_ref().unwrap().id, 2);
    let raw = content_links::query(Postgrest::new("http://localhost").unwrap())
        .select(rp_supabase_client::schema::named::<_, Linked>())
        .embedded(Linked::article, |child| {
            child.eq(articles::columns::owner_id, &9);
        })
        .embedded(Linked::video, |child| {
            child.eq(videos::columns::owner_id, &9);
        })
        .into_raw();
    let pairs: Vec<_> = raw.query_pairs().collect();
    assert!(pairs.contains(&("article.owner_id", "eq.9")));
    assert!(pairs.contains(&("video.owner_id", "eq.9")));
    assert!(pairs.contains(&("select", "id,article:articles!links_article(id,name,owner_id),video:videos!links_video(id,name,owner_id)")));
}

#[test]
fn one_dto_has_two_exact_relation_contracts() {
    assert_eq!(
        <Content as Projection<articles::Row>>::selection(),
        "id,name,owner_id"
    );
    assert_eq!(
        <Content as Projection<videos::Row>>::selection(),
        "id,name,owner_id"
    );
    let row: Content = serde_json::from_str(r#"{"id":1,"name":"same","owner_id":9}"#).unwrap();
    assert_eq!((row.id, row.name.as_str(), row.owner_id), (1, "same", 9));
}

#[tokio::test]
async fn typed_operations_and_pure_pairs_share_wire_grammar() {
    let mut server = mockito::Server::new_async().await;
    let client = Postgrest::new(server.url()).unwrap();
    let values = ["comma,value", "quote\"slash\\", "plain"];
    let typed = articles::query(client)
        .select(rp_supabase_client::schema::named::<_, Content>())
        .order_with_nulls(articles::columns::name, Order::Asc, Nulls::Last)
        .order(articles::columns::id, Order::Desc)
        .in_(articles::columns::name, values)
        .json_text_eq(
            articles::columns::document,
            &["nested.key", "slug"],
            "literal,\"\\",
        )
        .unwrap()
        .limit(3)
        .range(1, 2)
        .into_raw();
    let pairs: Vec<_> = typed
        .query_pairs()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    assert!(pairs.contains(&("order".into(), "name.asc.nullslast,id.desc".into())));
    assert!(pairs.contains(&("limit".into(), "3".into())));
    let pure = vec![
        params::projection::<articles::Row, Content>(),
        params::in_(articles::columns::name, values),
        params::json_text_eq(
            articles::columns::document,
            &["nested.key", "slug"],
            "literal,\"\\",
        )
        .unwrap(),
    ];
    for (key, value) in &pure {
        assert!(pairs.contains(&(key.to_string(), value.to_string())));
    }
    assert_eq!(pure[1].1, r#"in.("comma,value","quote\"slash\\","plain")"#);
    assert_eq!(pure[2].0, "document->\"nested.key\"->>slug");
    assert_eq!(pure[2].1, "eq.literal,\"\\");
    let response = server
        .mock("GET", "/articles")
        .match_query(mockito::Matcher::AllOf(
            pure.iter()
                .map(|(k, v)| mockito::Matcher::UrlEncoded(k.to_string(), v.to_string()))
                .collect(),
        ))
        .match_header("range-unit", "items")
        .match_header("range", "1-2")
        .with_body(r#"[{"id":1,"name":"same","owner_id":9}]"#)
        .expect(2)
        .create_async()
        .await;
    let raw: Vec<Content> = typed.fetch().await.unwrap();
    let independent: Vec<Content> = rp_supabase_client::rp_postgrest::reqwest::Client::new()
        .get(format!("{}/articles", server.url()))
        .header("range-unit", "items")
        .header("range", "1-2")
        .query(&pure)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(raw, independent);
    response.assert_async().await;
}

#[tokio::test]
async fn headers_supply_counts_and_minimal_writes_decode_no_rows() {
    let mut server = mockito::Server::new_async().await;
    let client = Postgrest::new(server.url()).unwrap();
    let head = server
        .mock("HEAD", "/articles")
        .match_header("prefer", "count=exact")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "1".into()),
            mockito::Matcher::UrlEncoded("owner_id".into(), "eq.9".into()),
        ]))
        .with_header("content-range", "0-0/42")
        .create_async()
        .await;
    assert_eq!(
        articles::query(client.clone())
            .eq(articles::columns::owner_id, &9)
            .limit(1)
            .count(Count::Exact)
            .await
            .unwrap(),
        42
    );
    head.assert_async().await;
    let counted = server
        .mock("GET", "/videos")
        .match_header("prefer", "count=exact")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "1".into()),
            mockito::Matcher::UrlEncoded("select".into(), "id,name,owner_id".into()),
        ]))
        .with_header("content-range", "0-0/7")
        .with_body(r#"[{"id":1,"name":"same","owner_id":9}]"#)
        .create_async()
        .await;
    let rows = videos::query(client.clone())
        .select(rp_supabase_client::schema::named::<_, Content>())
        .limit(1)
        .fetch_with_count(Count::Exact)
        .await
        .unwrap();
    assert_eq!(rows.count, 7);
    assert_eq!(rows.data[0].name, "same");
    counted.assert_async().await;
    let minimal = server
        .mock("DELETE", "/articles")
        .match_header("prefer", "return=minimal")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("id".into(), "eq.1".into()),
            mockito::Matcher::UrlEncoded("select".into(), "*".into()),
        ]))
        .with_status(204)
        .create_async()
        .await;
    articles::query(client.clone())
        .eq(articles::columns::id, &1)
        .delete()
        .execute()
        .await
        .unwrap();
    minimal.assert_async().await;
    let affected = server
        .mock("DELETE", "/videos")
        .match_header("prefer", mockito::Matcher::Regex("return=minimal".into()))
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("owner_id".into(), "eq.9".into()),
            mockito::Matcher::UrlEncoded("select".into(), "*".into()),
        ]))
        .with_header("content-range", "*/3")
        .with_status(204)
        .create_async()
        .await;
    assert_eq!(
        videos::query(client)
            .eq(videos::columns::owner_id, &9)
            .delete()
            .execute_with_count(Count::Exact)
            .await
            .unwrap(),
        3
    );
    affected.assert_async().await;
}

#[tokio::test]
async fn canonical_body_is_available_without_destructuring() {
    let mut server = mockito::Server::new_async().await;
    let failure = server.mock("GET", "/articles").with_status(400)
        .match_query(mockito::Matcher::UrlEncoded("select".into(), "id,name,owner_id".into()))
        .with_body(r#"{"code":"PGRST100","message":"bad query","details":"parse detail","hint":"try again"}"#).create_async().await;
    let error = articles::query(Postgrest::new(server.url()).unwrap())
        .select(rp_supabase_client::schema::named::<_, Content>())
        .fetch()
        .await
        .unwrap_err();
    let body = error.postgrest_body().unwrap();
    assert_eq!(
        serde_json::to_value(body).unwrap(),
        serde_json::json!({"code":"PGRST100","message":"bad query","details":"parse detail","hint":"try again"})
    );
    assert!(error.postgrest_error().is_some());
    failure.assert_async().await;
}

#[tokio::test]
async fn zero_limit_and_empty_in_are_literal_read_requests() {
    let mut server = mockito::Server::new_async().await;
    let empty = server
        .mock("GET", "/articles")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "0".into()),
            mockito::Matcher::UrlEncoded("id".into(), "in.()".into()),
        ]))
        .with_body("[]")
        .create_async()
        .await;
    let rows = articles::query(Postgrest::new(server.url()).unwrap())
        .select(rp_supabase_client::schema::named::<_, Content>())
        .in_(articles::columns::id, core::iter::empty::<&i64>())
        .limit(0)
        .fetch()
        .await
        .unwrap();
    assert!(rows.is_empty());
    empty.assert_async().await;
}
