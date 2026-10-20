#[allow(dead_code)]
mod database {
    rp_supabase_client::include_schema!("database.rs");
}
use database::public::tables::{articles, videos};
use rp_supabase_client::rp_postgrest::Postgrest;
use rp_supabase_client::schema::{
    Column, FilterColumn, Projection, Query, Relation, SharedFilter, named, params,
};
use rp_supabase_client::{key, projection};

projection! {
    #[derive(Debug, PartialEq)]
    struct Content for [database::public::tables::articles, database::public::tables::videos] {
        name,
    } filters {
        content_id: [id, owner_id],
        owner: [owner_id, id],
    }
}

fn pair<R: Relation>(id: i64) -> params::QueryPair
where
    Content: FilterColumn<key!(type content_id), R>,
    SharedFilter<Content, key!(type content_id), R>: Column<Relation = R, Filter = i64>,
{
    params::eq(Content::content_id::<R>(), &id)
}

fn filtered<R: Relation>(query: Query<R, Content>, id: i64) -> Query<R, Content>
where
    Content: Projection<R> + FilterColumn<key!(type content_id), R>,
    SharedFilter<Content, key!(type content_id), R>: Column<Relation = R, Filter = i64>,
{
    query.eq(Content::content_id::<R>(), &id)
}

#[test]
fn generic_shared_keys_map_columns_without_selecting_them() {
    assert_eq!(pair::<articles::Row>(7), ("id".into(), "eq.7".into()));
    assert_eq!(pair::<videos::Row>(7), ("owner_id".into(), "eq.7".into()));
    assert_eq!(
        params::eq(Content::owner::<articles::Row>(), &8),
        ("owner_id".into(), "eq.8".into())
    );
    assert_eq!(
        params::eq(Content::owner::<videos::Row>(), &8),
        ("id".into(), "eq.8".into())
    );
    assert_eq!(<Content as Projection<articles::Row>>::selection(), "name");
    assert_eq!(<Content as Projection<videos::Row>>::selection(), "name");
    let record: Content = serde_json::from_str(r#"{"name":"search"}"#).unwrap();
    assert_eq!(record.name, "search");
    assert!(serde_json::from_str::<Content>(r#"{"id":7}"#).is_err());
}

#[tokio::test]
async fn generic_query_uses_each_relations_shared_column() {
    let mut server = mockito::Server::new_async().await;
    let article = server
        .mock("GET", "/articles")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "name".into()),
            mockito::Matcher::UrlEncoded("id".into(), "eq.7".into()),
        ]))
        .with_body(r#"[{"name":"article"}]"#)
        .create_async()
        .await;
    let video = server
        .mock("GET", "/videos")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("select".into(), "name".into()),
            mockito::Matcher::UrlEncoded("owner_id".into(), "eq.7".into()),
        ]))
        .with_body(r#"[{"name":"video"}]"#)
        .create_async()
        .await;
    let client = Postgrest::new(server.url()).unwrap();
    let article_rows = filtered(
        articles::query(client.clone()).select(named::<_, Content>()),
        7,
    )
    .fetch()
    .await
    .unwrap();
    let video_rows = filtered(videos::query(client).select(named::<_, Content>()), 7)
        .fetch()
        .await
        .unwrap();
    assert_eq!(article_rows[0].name, "article");
    assert_eq!(video_rows[0].name, "video");
    article.assert_async().await;
    video.assert_async().await;
}
