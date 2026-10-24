#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "Selected live acceptance tests fail with setup context"
)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Live integration test crate"
)]

use rp_supabase_graphql::GraphqlClient;
use serde_json::{Value, json};
use url::Url;

/// Requires the parent-managed `graphql_probe` fixture; fails on missing setup.
#[tokio::test]
#[ignore = "requires local Supabase with graphql_probe fixture and Supabase environment variables"]
async fn mutation_variables_readback_and_anonymous_rls() {
    let url = Url::parse(&std::env::var("SUPABASE_URL").expect("SUPABASE_URL required")).unwrap();
    let anon = std::env::var("SUPABASE_ANON_KEY").expect("SUPABASE_ANON_KEY required");
    let service =
        std::env::var("SUPABASE_SERVICE_ROLE_KEY").expect("SUPABASE_SERVICE_ROLE_KEY required");
    let admin = GraphqlClient::new(&url, &service).unwrap();
    let inserted = admin.query("mutation InsertProbe($value: String!) { insertIntoGraphqlProbeCollection(objects: [{value: $value}]) { records { id value } } }")
        .variables(&json!({"value":"graphql-sdk-live"})).operation_name("InsertProbe")
        .fetch::<Value>().await.unwrap().into_result().unwrap();
    let id =
        inserted.data.as_ref().unwrap()["insertIntoGraphqlProbeCollection"]["records"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
    let document = "query ReadProbe($id: UUID!) { graphqlProbeCollection(filter: {id: {eq: $id}}) { edges { node { id value } } } }";
    let response = admin
        .query(document)
        .variables(&json!({"id":id}))
        .operation_name("ReadProbe")
        .fetch::<Value>()
        .await
        .unwrap()
        .into_result()
        .unwrap();
    assert_eq!(
        response.data.as_ref().unwrap()["graphqlProbeCollection"]["edges"][0]["node"]["value"],
        "graphql-sdk-live"
    );
    let anonymous = GraphqlClient::new(&url, &anon)
        .unwrap()
        .query(document)
        .variables(&json!({"id":id}))
        .fetch::<Value>()
        .await
        .unwrap()
        .into_result()
        .unwrap();
    assert_eq!(
        anonymous.data.as_ref().unwrap()["graphqlProbeCollection"]["edges"],
        json!([])
    );
    let deleted = admin.query("mutation DeleteProbe($id: UUID!) { deleteFromGraphqlProbeCollection(filter: {id: {eq: $id}}, atMost: 1) { affectedCount } }")
        .variables(&json!({"id":id})).fetch::<Value>().await.unwrap().into_result().unwrap();
    assert_eq!(
        deleted.data.as_ref().unwrap()["deleteFromGraphqlProbeCollection"]["affectedCount"],
        1_i32
    );
}
