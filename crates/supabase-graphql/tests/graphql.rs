#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "Protocol integration tests fail with assertion context"
)]
#![expect(clippy::tests_outside_test_module, reason = "Integration test crate")]

use mockito::Matcher;
use rp_supabase_graphql::{GraphqlClient, GraphqlClientError, GraphqlPathSegment, RetryPolicy};
use serde_json::{Value, json};
use url::Url;

fn client(server: &mockito::Server) -> GraphqlClient {
    GraphqlClient::new(&Url::parse(&server.url()).unwrap(), "legacy-secret").unwrap()
}

#[tokio::test]
async fn variables_operation_partial_data_and_metadata() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("POST", "/graphql/v1")
        .match_header("apikey", "legacy-secret").match_header("authorization", "Bearer user-secret")
        .match_header("x-custom", "custom-secret")
        .match_body(Matcher::Json(json!({"query":"query Read($id: Int!) { item(id: $id) { name } }", "variables":{"id":7_i32}, "operationName":"Read"})))
        .with_body(r#"{"data":{"item":null},"errors":[{"message":"denied","locations":[{"line":1,"column":30}],"path":["item",0,"name"],"extensions":{"code":"DENIED"}}],"extensions":{"trace":"a"}}"#)
        .create_async().await;
    let builder = client(&server)
        .with_access_token("user-secret")
        .unwrap()
        .query("query Read($id: Int!) { item(id: $id) { name } }")
        .variables(&json!({"id":7_i32}))
        .operation_name("Read")
        .header("x-custom", "custom-secret");
    let debug = format!("{builder:?}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("Read"));
    let response = builder.fetch::<Value>().await.unwrap();
    assert_eq!(response.data, Some(json!({"item":null})));
    assert_eq!(response.errors[0].path[1], GraphqlPathSegment::Index(0));
    assert_eq!(response.errors[0].locations[0].column, 30);
    assert_eq!(
        response.errors[0].extensions,
        Some(json!({"code":"DENIED"}))
    );
    assert_eq!(response.extensions, Some(json!({"trace":"a"})));
    let rejected = response.into_result().unwrap_err();
    assert_eq!(rejected.errors[0].message, "denied");
    mock.assert_async().await;
}

#[tokio::test]
async fn null_missing_empty_and_malformed_envelopes() {
    let mut server = mockito::Server::new_async().await;
    for body in [
        r#"{"data":null,"errors":[{"message":"bad"}]}"#,
        r#"{"errors":[{"message":"bad"}]}"#,
    ] {
        let mock = server
            .mock("POST", "/graphql/v1")
            .with_body(body)
            .create_async()
            .await;
        let response = client(&server).query("bad").fetch::<Value>().await.unwrap();
        assert!(response.data.is_none());
        assert_eq!(response.errors.len(), 1);
        mock.assert_async().await;
        mock.remove_async().await;
    }
    let mock = server
        .mock("POST", "/graphql/v1")
        .with_body(r#"{"data":null}"#)
        .create_async()
        .await;
    assert!(
        client(&server)
            .query("x")
            .fetch::<Value>()
            .await
            .unwrap()
            .data
            .is_none()
    );
    mock.remove_async().await;
    for body in ["{}", r#"{"errors":[]}"#] {
        let mock = server
            .mock("POST", "/graphql/v1")
            .with_body(body)
            .create_async()
            .await;
        assert!(matches!(
            client(&server).query("x").fetch::<Value>().await,
            Err(GraphqlClientError::EmptyEnvelope)
        ));
        mock.remove_async().await;
    }
    let _mock = server
        .mock("POST", "/graphql/v1")
        .with_body(r#"{"data":42}"#)
        .create_async()
        .await;
    assert!(matches!(
        client(&server).query("x").fetch::<String>().await,
        Err(GraphqlClientError::Decode(_))
    ));
}

#[tokio::test]
async fn http_errors_and_post_retry_opt_in() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/graphql/v1")
        .with_status(429)
        .with_body("limited")
        .expect(1)
        .create_async()
        .await;
    let result = client(&server).query("mutation { deleteAll }").send().await;
    assert!(
        matches!(result, Err(GraphqlClientError::Http { status, body }) if status.as_u16() == 429 && body == "limited")
    );
    mock.assert_async().await;
    mock.remove_async().await;
    let mock = server
        .mock("POST", "/graphql/v1")
        .with_status(429)
        .expect(2)
        .create_async()
        .await;
    let policy = RetryPolicy::new(1, core::time::Duration::ZERO, core::time::Duration::ZERO)
        .retry_non_idempotent(true);
    let _result = client(&server)
        .with_retry_policy(policy)
        .with_access_token("user")
        .unwrap()
        .query("query { x }")
        .send()
        .await;
    mock.assert_async().await;
}

#[tokio::test]
async fn new_keys_and_sensitive_client_debug() {
    let mut server = mockito::Server::new_async().await;
    for key in ["sb_publishable_secret", "sb_secret_hidden"] {
        let mock = server
            .mock("POST", "/graphql/v1")
            .match_header("apikey", key)
            .match_header("authorization", Matcher::Missing)
            .with_body("raw response")
            .create_async()
            .await;
        let client = GraphqlClient::new(&Url::parse(&server.url()).unwrap(), key).unwrap();
        assert!(!format!("{client:?}").contains(key));
        assert_eq!(
            client
                .query("query { x }")
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "raw response"
        );
        mock.assert_async().await;
        mock.remove_async().await;
    }
}

#[tokio::test]
async fn redirects_stay_on_origin_by_default_and_custom_clients_are_preserved() {
    let mut origin = mockito::Server::new_async().await;
    let mut other = mockito::Server::new_async().await;
    let leaked = other.mock("POST", "/sink").expect(0).create_async().await;
    let redirect = origin
        .mock("POST", "/graphql/v1")
        .with_status(307)
        .with_header("location", &format!("{}/sink", other.url()))
        .create_async()
        .await;
    let blocked = client(&origin)
        .query("query { x }")
        .send()
        .await
        .unwrap_err();
    assert!(matches!(
        blocked,
        GraphqlClientError::Http {
            status: reqwest::StatusCode::TEMPORARY_REDIRECT,
            ..
        }
    ));
    leaked.assert_async().await;
    redirect.remove_async().await;
    let redirect = origin
        .mock("POST", "/graphql/v1")
        .with_status(307)
        .with_header("location", "/next")
        .create_async()
        .await;
    let next = origin
        .mock("POST", "/next")
        .match_header("apikey", "legacy-secret")
        .with_body("ok")
        .create_async()
        .await;
    assert_eq!(
        client(&origin)
            .query("x")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    next.assert_async().await;
    redirect.remove_async().await;
    let _redirect = origin
        .mock("POST", "/graphql/v1")
        .with_status(307)
        .with_header("location", "/next")
        .create_async()
        .await;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let custom =
        GraphqlClient::new_with_client(&Url::parse(&origin.url()).unwrap(), "key", http).unwrap();
    assert!(
        matches!(custom.query("x").send().await, Err(GraphqlClientError::Http { status, .. }) if status.as_u16() == 307)
    );
}

#[tokio::test]
async fn regression_raw_response_decodes_standard_success_envelope() {
    let mut server = mockito::Server::new_async().await;
    let success = server
        .mock("POST", "/graphql/v1")
        .with_body(r#"{"data":{"answer":42}}"#)
        .create_async()
        .await;
    let response = client(&server)
        .query("{ answer }")
        .send()
        .await
        .unwrap()
        .json::<rp_supabase_graphql::GraphqlResponse<Value>>()
        .await
        .unwrap();
    assert_eq!(response.data, Some(json!({"answer":42_i32})));
    assert_eq!(response.errors, Vec::new());
    success.assert_async().await;
}
