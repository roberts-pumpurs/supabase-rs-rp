#![expect(clippy::tests_outside_test_module, reason = "Integration test crate")]
#![expect(
    clippy::unwrap_used,
    reason = "Tests report setup failures with assertion context"
)]
use rp_supabase_http::{RetryPolicy, execute};

#[tokio::test]
async fn read_recovers_after_rate_limit() {
    let mut server = mockito::Server::new_async().await;
    let limited = server
        .mock("GET", "/recover")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let recovered = server
        .mock("GET", "/recover")
        .with_status(200)
        .with_body("ready")
        .expect(1)
        .create_async()
        .await;
    let http = reqwest::Client::new();
    let response = execute(
        &http,
        http.get(format!("{}/recover", server.url()))
            .build()
            .unwrap(),
        RetryPolicy::default(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ready");
    limited.assert_async().await;
    recovered.assert_async().await;
}

#[tokio::test]
async fn streaming_mutation_is_sent_once_even_with_replay_permission() {
    let mut server = mockito::Server::new_async().await;
    let limited = server
        .mock("POST", "/stream")
        .match_body("payload")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let body = reqwest::Body::wrap_stream(futures_util::stream::once(async {
        Ok::<_, std::io::Error>(b"payload".as_slice())
    }));
    let http = reqwest::Client::new();
    let response = execute(
        &http,
        http.post(format!("{}/stream", server.url()))
            .body(body)
            .build()
            .unwrap(),
        RetryPolicy::default().retry_non_idempotent(true),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 429);
    limited.assert_async().await;
}
