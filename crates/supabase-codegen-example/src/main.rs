//! Offline by default. Set SUPABASE_CODEGEN_API_URL to a PostgREST base URL
//! (including /rest/v1 for Supabase) to run finite live CRUD/RPC and relationship scenarios.
//! Apply smoke.sql to a disposable database first. For live code generation set
//! SUPABASE_CODEGEN_DATABASE_URL in the build environment. Optional API credentials
//! come only from SUPABASE_CODEGEN_API_KEY and SUPABASE_CODEGEN_ACCESS_TOKEN.

use rp_supabase_client::rp_postgrest::Postgrest;
use rp_supabase_client::schema::{Array, Field, Projection, Selection, rpc};
use serde_json::json;

rp_supabase_client::include_schema!("database.rs");

mod gaps;
#[allow(dead_code)]
mod relationship_projections;
mod relationships;

use public::functions::echo_message;
use public::tables::messages::{self, Insert, Update};

fn insert(body: &str) -> Insert {
    // The custom TypedBuilder derive is applied only to this generated struct.
    Insert::builder()
        .body(body.to_owned())
        .note(Field::Value(None))
        .created_at(Field::Omit)
        .mood(Field::Omit)
        .amount(Field::Value(
            serde_json::from_str("123456789012345678901234567890.123456789")
                .expect("valid numeric fixture"),
        ))
        .metadata(Field::Value(
            json!({"source": "codegen-example"})
                .try_into()
                .expect("non-null metadata"),
        ))
        .tags(Field::Value(Array::Elements(vec![
            Some("generated".into()),
            None,
        ])))
        .build()
}

fn offline() -> Result<(), Box<dyn std::error::Error>> {
    use public::tables::messages::Row as Root;
    use rp_supabase_client::select;
    let selected = select!(Root => { id, body, note, amount });
    let request = serde_json::to_value(insert("offline example"))?;
    assert_eq!(request["note"], json!(null));
    assert!(request.get("created_at").is_none());
    assert!(request.get("id").is_none());
    let rows = selected.decode(r#"[{"id":1,"body":"offline example","note":null,"amount":123456789012345678901234567890.123456789}]"#)?;
    let row = &rows[0];
    assert_eq!(row.note, None);
    assert_eq!(row.body, "offline example");
    assert_eq!(
        row.amount.to_string(),
        "123456789012345678901234567890.123456789"
    );
    println!(
        "Offline generated bindings: {}",
        serde_json::to_string(&request)?
    );
    fn rendered<S: Selection>(_: S) -> String {
        <S::Record as Projection<S::Relation>>::selection().into_owned()
    }
    // Computed fields are sent only when selected; PostgREST omits them from `*`.
    let computed = rendered(select!(Root => { id, excerpt, message_meta }));
    assert_eq!(computed, "id,excerpt,message_meta");
    assert_eq!(<Root as Projection<Root>>::selection(), "*");
    relationship_projections::offline()?;
    Ok(())
}

async fn live(client: Postgrest) -> Result<(), Box<dyn std::error::Error>> {
    use public::tables::messages::Row as Root;
    use rp_supabase_client::select;
    let selected = select!(Root => { id, body, note, amount });
    relationships::live(client.clone()).await?;
    gaps::live(client.clone()).await?;
    let rows = selected
        .query(client.clone())
        .insert(&insert("live example"))
        .fetch()
        .await?;
    let row = rows.first().ok_or("insert returned no row")?;
    let id = row.id;
    // Attempt cleanup after insertion, including recoverable scenario errors.
    let scenario = async {
        let rows = selected
            .query(client.clone())
            .eq(messages::columns::id, &id)
            .fetch()
            .await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].note, None);
        let update = Update {
            note: Field::Value(Some("updated".into())),
            ..Update::default()
        };
        assert_eq!(
            rows[0].amount.to_string(),
            "123456789012345678901234567890.123456789"
        );
        let rows = selected
            .query(client.clone())
            .eq(messages::columns::id, &id)
            .update(&update)
            .fetch()
            .await?;
        assert_eq!(rows[0].note.as_deref(), Some("updated"));
        let views = public::tables::message_summaries::query(client.clone())
            .eq(public::tables::message_summaries::columns::id, &id)
            .fetch()
            .await?;
        assert_eq!(views.len(), 1);
        let args = echo_message::Args {
            message: Some("rpc example".into()),
        };
        let echoed = rpc::<echo_message::Function>(client.clone(), &args)
            .fetch()
            .await?;
        assert_eq!(echoed.as_deref(), Some("rpc example"));
        let computed = select!(Root => { id, excerpt, message_meta })
            .query(client.clone())
            .eq(messages::columns::excerpt, "live example")
            .order(
                messages::columns::excerpt,
                rp_supabase_client::schema::Order::Desc,
            )
            .fetch()
            .await?;
        let row = computed
            .iter()
            .find(|row| row.id == id)
            .ok_or("computed field filter missed row")?;
        assert_eq!(row.excerpt.as_deref(), Some("live example"));
        assert_eq!(
            row.message_meta.as_ref().map(|meta| &meta["id"]),
            Some(&json!(id))
        );
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    let cleanup = messages::query(client)
        .eq(messages::columns::id, &id)
        .delete()
        .fetch()
        .await;
    scenario?;
    assert_eq!(cleanup?.len(), 1);
    println!("Live insert/select/update/view/RPC/delete scenario passed");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    offline()?;
    if let Ok(url) = std::env::var("SUPABASE_CODEGEN_API_URL") {
        let mut client = Postgrest::new(url.trim_end_matches('/'))?;
        if let Ok(key) = std::env::var("SUPABASE_CODEGEN_API_KEY") {
            client = client.insert_header("apikey", key)?;
        }
        if let Ok(token) = std::env::var("SUPABASE_CODEGEN_ACCESS_TOKEN") {
            client = client.auth(token)?;
        }
        tokio::time::timeout(std::time::Duration::from_secs(30), live(client)).await??;
    }
    Ok(())
}
