#![expect(clippy::tests_outside_test_module, reason = "Integration test crate")]
#![expect(
    clippy::unwrap_used,
    reason = "Tests report setup failures with assertion context"
)]
use core::time::Duration;
use rp_supabase_http::{RetryPolicy, execute};

#[test]
fn execution_future_keeps_retry_state_off_the_caller_stack() {
    let http = reqwest::Client::new();
    let request = http.get("http://localhost/resource").build().unwrap();
    let future = execute(&http, request, RetryPolicy::default());
    assert!(
        core::mem::size_of_val(&future) <= 64,
        "HTTP execution must not embed the retry state in its caller"
    );
}

#[tokio::test]
async fn rate_limited_read_stops_at_budget_and_keeps_final_response() {
    let mut server = mockito::Server::new_async().await;
    let limited = server
        .mock("GET", "/resource")
        .with_status(429)
        .with_header("retry-after", "0")
        .with_header("x-limit", "reached")
        .with_body("limited")
        .expect(3)
        .create_async()
        .await;
    let http = reqwest::Client::new();
    let response = execute(
        &http,
        http.get(format!("{}/resource", server.url()))
            .build()
            .unwrap(),
        RetryPolicy::default(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["x-limit"], "reached");
    assert_eq!(response.text().await.unwrap(), "limited");
    limited.assert_async().await;
}

#[tokio::test]
async fn mutations_require_explicit_replay_permission() {
    let mut server = mockito::Server::new_async().await;
    let once = server
        .mock("POST", "/once")
        .match_body("payload")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let replay = server
        .mock("POST", "/replay")
        .match_body("payload")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(3)
        .create_async()
        .await;
    let http = reqwest::Client::new();
    for (path, policy) in [
        ("/once", RetryPolicy::default()),
        ("/replay", RetryPolicy::default().retry_non_idempotent(true)),
    ] {
        let response = execute(
            &http,
            http.post(format!("{}{path}", server.url()))
                .body("payload")
                .build()
                .unwrap(),
            policy,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 429);
    }
    once.assert_async().await;
    replay.assert_async().await;
}

#[tokio::test]
async fn long_retry_after_is_not_shortened_and_disabled_policy_does_not_replay() {
    let mut server = mockito::Server::new_async().await;
    let http = reqwest::Client::new();
    for (path, after, policy) in [
        ("/seconds", "3600", RetryPolicy::default()),
        (
            "/overflow",
            "999999999999999999999999999999999999999",
            RetryPolicy::default(),
        ),
        (
            "/date",
            "Fri, 01 Jan 2100 00:00:00 GMT",
            RetryPolicy::default(),
        ),
        ("/disabled", "0", RetryPolicy::none()),
    ] {
        let mock = server
            .mock("GET", path)
            .with_status(429)
            .with_header("retry-after", after)
            .with_body(path)
            .expect(1)
            .create_async()
            .await;
        let response = execute(
            &http,
            http.get(format!("{}{path}", server.url())).build().unwrap(),
            policy,
        )
        .await
        .unwrap();
        assert_eq!(response.text().await.unwrap(), path);
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn service_failures_are_not_replayed() {
    let mut server = mockito::Server::new_async().await;
    let failed = server
        .mock("GET", "/failure")
        .with_status(503)
        .with_header("retry-after", "0")
        .with_body("unavailable")
        .expect(1)
        .create_async()
        .await;
    let http = reqwest::Client::new();
    let response = execute(
        &http,
        http.get(format!("{}/failure", server.url()))
            .build()
            .unwrap(),
        RetryPolicy::new(2, Duration::ZERO, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(response.text().await.unwrap(), "unavailable");
    failed.assert_async().await;
}
