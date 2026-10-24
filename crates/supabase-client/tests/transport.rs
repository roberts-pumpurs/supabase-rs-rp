#![cfg(feature = "client")]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Integration tests exercise public interfaces."
)]
#![expect(
    clippy::unwrap_used,
    reason = "Test setup and assertions retain failure details."
)]

use core::time::Duration;
use futures::StreamExt as _;
use mockito::Matcher;
use rp_supabase_client::{
    anonymous_client, anonymous_client_with_client, new_authenticated,
    new_authenticated_with_client,
    rp_postgrest::reqwest,
    rp_supabase_auth::{jwt_stream::SupabaseAuthConfig, types::LoginCredentials, url::Url},
};

fn configured_http() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-transport",
        reqwest::header::HeaderValue::from_static("retained"),
    );
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_static("Bearer caller-default"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
}

fn config(url: &str) -> SupabaseAuthConfig {
    SupabaseAuthConfig {
        api_key: "key".into(),
        url: Url::parse(url).unwrap(),
        max_reconnect_attempts: 1,
        reconnect_interval: Duration::from_millis(1),
    }
}

fn credentials() -> LoginCredentials {
    LoginCredentials::email("user@example.com".into(), "password".into())
}

#[tokio::test]
async fn anonymous_default_and_configured_headers_and_redirect_policy() {
    let mut server = mockito::Server::new_async().await;
    let url = Url::parse(&server.url()).unwrap();
    let default = server
        .mock("GET", "/rest/v1/default")
        .match_header("apikey", "key")
        .with_body("[]")
        .create_async()
        .await;
    let rows: Vec<i32> = anonymous_client("key".into(), &url)
        .unwrap()
        .from("default")
        .fetch()
        .await
        .unwrap();
    assert!(rows.is_empty());
    default.assert_async().await;
    let configured = server
        .mock("GET", "/rest/v1/configured")
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer caller-default")
        .with_status(302)
        .with_header("location", "/unexpected")
        .create_async()
        .await;
    let response = anonymous_client_with_client("key".into(), &url, configured_http())
        .unwrap()
        .from("configured")
        .execute()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    configured.assert_async().await;
}

#[tokio::test]
async fn authenticated_login_refresh_and_rest_share_configured_transport() {
    let mut server = mockito::Server::new_async().await;
    let login = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer caller-default")
        .match_header("content-type", "application/json")
        .match_header("accept", "application/json")
        .match_body(Matcher::PartialJson(
            serde_json::json!({"email":"user@example.com","password":"password"}),
        ))
        .with_body(r#"{"access_token":"first","refresh_token":"refresh","expires_in":0}"#)
        .create_async()
        .await;
    let refresh = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded(
            "grant_type".into(),
            "refresh_token".into(),
        ))
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer caller-default")
        .match_header("content-type", "application/json")
        .match_body(Matcher::PartialJson(
            serde_json::json!({"refresh_token":"refresh"}),
        ))
        .with_body(r#"{"access_token":"second","refresh_token":"rotated","expires_in":3600}"#)
        .create_async()
        .await;
    let first_rest = server
        .mock("GET", "/rest/v1/first")
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer first")
        .with_body("[]")
        .create_async()
        .await;
    let second_rest = server
        .mock("GET", "/rest/v1/second")
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer second")
        .with_body("[]")
        .create_async()
        .await;
    let stream =
        new_authenticated_with_client(config(&server.url()), credentials(), configured_http())
            .unwrap();
    futures::pin_mut!(stream);
    let (first, _) = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let rows: Vec<i32> = first.from("first").fetch().await.unwrap();
    assert!(rows.is_empty());
    let (second, _) = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let rows: Vec<i32> = second.from("second").fetch().await.unwrap();
    assert!(rows.is_empty());
    login.assert_async().await;
    refresh.assert_async().await;
    first_rest.assert_async().await;
    second_rest.assert_async().await;
}

#[tokio::test]
async fn authenticated_default_constructor_supplies_protocol_headers() {
    let mut server = mockito::Server::new_async().await;
    let login = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("apikey", "key")
        .match_header("content-type", "application/json")
        .with_body(r#"{"access_token":"token","refresh_token":"refresh","expires_in":3600}"#)
        .create_async()
        .await;
    let rest = server
        .mock("GET", "/rest/v1/rows")
        .match_header("apikey", "key")
        .match_header("authorization", "Bearer token")
        .with_body("[]")
        .create_async()
        .await;
    let stream = new_authenticated(config(&server.url()), credentials()).unwrap();
    futures::pin_mut!(stream);
    let (client, _) = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let rows: Vec<i32> = client.from("rows").fetch().await.unwrap();
    assert!(rows.is_empty());
    login.assert_async().await;
    rest.assert_async().await;
}

#[expect(
    clippy::panic,
    reason = "An unexpected error fails this configured timeout contract"
)]
#[tokio::test]
async fn configured_anonymous_client_preserves_total_timeout() {
    let mut server = mockito::Server::new_async().await;
    let _slow = server
        .mock("GET", "/rest/v1/slow")
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(150));
            writer.write_all(b"[]")
        })
        .create_async()
        .await;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let client =
        anonymous_client_with_client("key".into(), &Url::parse(&server.url()).unwrap(), http)
            .unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        client.from("slow").fetch::<Vec<i32>>(),
    )
    .await
    .unwrap()
    .unwrap_err();
    let (rp_supabase_client::Error::Request(source)
    | rp_supabase_client::Error::ResponseBody { source, .. }) = error
    else {
        panic!("expected configured timeout");
    };
    assert!(source.is_timeout());
}
