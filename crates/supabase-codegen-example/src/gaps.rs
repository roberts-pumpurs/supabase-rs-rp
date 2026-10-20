use crate::public::tables::{articles, videos};
use rp_supabase_client::{
    rp_postgrest::{Count, Postgrest},
    schema::{Order, params},
};

rp_supabase_client::projection! {
    #[derive(Debug, PartialEq)]
    struct Content for [crate::public::tables::articles, crate::public::tables::videos] { id, name, owner_id }
}

pub async fn live(client: Postgrest) -> Result<(), Box<dyn std::error::Error>> {
    let id = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_micros(),
    )?;
    let name = format!("gap,{id}\"\\");
    let scenario = async {
        articles::query(client.clone())
            .insert(&articles::Insert {
                id,
                name: name.clone(),
                owner_id: id,
                document: serde_json::json!({"slug":name}).try_into()?,
            })
            .execute()
            .await?;
        videos::query(client.clone())
            .insert(&videos::Insert {
                id,
                name: name.clone(),
                owner_id: id,
                document: serde_json::json!({"slug":name}).try_into()?,
            })
            .execute()
            .await?;
        let article = articles::query(client.clone())
            .select(rp_supabase_client::schema::named::<_, Content>())
            .in_(articles::columns::name, [name.as_str()])
            .json_text_eq(articles::columns::document, &["slug"], &name)?
            .order(articles::columns::id, Order::Desc)
            .range(0, 0)
            .fetch_with_count(Count::Exact)
            .await?;
        assert_eq!(article.count, 1);
        assert_eq!(article.data.len(), 1);
        let video: Vec<Content> = videos::query(client.clone())
            .select(rp_supabase_client::schema::named::<_, Content>())
            .eq(videos::columns::id, &id)
            .into_raw()
            .fetch()
            .await?;
        assert_eq!(article.data, video);
        assert_eq!(
            videos::query(client.clone())
                .eq(videos::columns::owner_id, &id)
                .limit(1)
                .count(Count::Exact)
                .await?,
            1
        );
        // Pure pairs can be fed to any reqwest client. Use the owned builder here
        // to retain the live runner's API credentials while proving literal grammar.
        let mut raw = client.from("articles");
        for (key, value) in [
            params::projection::<articles::Row, Content>(),
            params::eq(articles::columns::id, &id),
        ] {
            raw.append_query(key, value);
        }
        assert_eq!(raw.fetch::<Vec<Content>>().await?, video);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    // Attempt both cleanup operations even if insertion or a read fails.
    let article_cleanup = articles::query(client.clone())
        .eq(articles::columns::id, &id)
        .delete()
        .execute_with_count(Count::Exact)
        .await;
    let video_cleanup = videos::query(client)
        .eq(videos::columns::id, &id)
        .delete()
        .execute()
        .await;
    scenario?;
    assert_eq!(article_cleanup?, 1);
    video_cleanup?;
    println!("Live shared DTO/order/IN/JSON/pagination/count/minimal/raw scenario passed");
    Ok(())
}
