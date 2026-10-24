#![cfg(all(feature = "rest", feature = "auth", feature = "functions"))]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Live integration tests exercise public client behavior"
)]
#![expect(
    clippy::unwrap_used,
    reason = "Selected acceptance tests fail loudly with setup context"
)]

use supabase_rp::Client;

#[expect(
    clippy::panic,
    reason = "Explicit live runs fail when required configuration is missing"
)]
fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("live acceptance requires {name}"))
}

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct Probe {
    id: String,
    value: String,
}
#[derive(serde::Serialize)]
struct Input<'a> {
    value: &'a str,
}

#[tokio::test]
#[ignore = "requires local Supabase and the product acceptance migration"]
async fn string_crud_respects_rls_and_keeps_typed_results() {
    let url = required("SUPABASE_URL");
    let admin = Client::new(&url, &required("SUPABASE_SERVICE_ROLE_KEY")).unwrap();
    let anon = Client::new(&url, &required("SUPABASE_ANON_KEY")).unwrap();
    let rows: Vec<Probe> = admin
        .from("graphql_probe")
        .insert_json(&Input { value: "before" })
        .select("id,value")
        .fetch()
        .await
        .unwrap();
    let created = rows.first().unwrap();
    assert_eq!(created.value, "before");
    let visible: Vec<Probe> = anon
        .from("graphql_probe")
        .select("id,value")
        .eq("id", &created.id)
        .fetch()
        .await
        .unwrap();
    assert_eq!(visible, Vec::<Probe>::new());
    let changed: Vec<Probe> = admin
        .from("graphql_probe")
        .update_json(&Input { value: "after" })
        .eq("id", &created.id)
        .select("id,value")
        .fetch()
        .await
        .unwrap();
    assert_eq!(
        changed,
        vec![Probe {
            id: created.id.clone(),
            value: "after".into()
        }]
    );
    let fetched: Vec<Probe> = admin
        .from("graphql_probe")
        .select("id,value")
        .eq("id", &created.id)
        .fetch()
        .await
        .unwrap();
    assert_eq!(fetched, changed);
    let removed: Vec<Probe> = admin
        .from("graphql_probe")
        .delete()
        .eq("id", &created.id)
        .select("id,value")
        .fetch()
        .await
        .unwrap();
    assert_eq!(removed, changed);
    let absent: Vec<Probe> = admin
        .from("graphql_probe")
        .select("id,value")
        .eq("id", &created.id)
        .fetch()
        .await
        .unwrap();
    assert_eq!(absent, Vec::<Probe>::new());
}

#[derive(serde::Serialize)]
struct SumInput {
    left: i64,
    right: i64,
}
#[derive(serde::Deserialize)]
struct SumOutput {
    sum: i64,
}
#[derive(serde::Serialize)]
struct StreamInput {
    mode: &'static str,
}

#[tokio::test]
#[ignore = "requires local Supabase serving the product-probe function"]
async fn edge_function_returns_json_and_streamed_body() {
    let client = Client::new(&required("SUPABASE_URL"), &required("SUPABASE_ANON_KEY")).unwrap();
    let result: SumOutput = client
        .functions()
        .invoke("product-probe")
        .json(&SumInput {
            left: 19,
            right: 23,
        })
        .fetch()
        .await
        .unwrap();
    assert_eq!(result.sum, 42);
    let mut response = client
        .functions()
        .invoke("product-probe")
        .json(&StreamInput { mode: "stream" })
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "application/x-ndjson");
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(
        body,
        b"{\"sequence\":0}\n{\"sequence\":1}\n{\"sequence\":2}\n"
    );
}
