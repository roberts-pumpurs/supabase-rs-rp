#![expect(
    clippy::tests_outside_test_module,
    reason = "Integration tests exercise public interfaces."
)]

// Adapted from rp-postgrest 2.1.0 (MIT OR Apache-2.0).
use core::future::pending;
use core::time::Duration;

use rp_postgrest::{Postgrest, reqwest};
use tokio::net::TcpListener;
use tokio::time::timeout;

enum QueryKind {
    From,
    Rpc,
}

#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_assert_message,
    reason = "Fixture setup and assertions enforce the injected transport timeout"
)]
async fn assert_supplied_client_timeout(query: QueryKind) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_sender, accepted_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (_connection, _) = listener.accept().await.unwrap();
        accepted_sender.send(()).unwrap();
        pending::<()>().await;
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    let postgrest = Postgrest::new_with_client(format!("http://{address}"), client).unwrap();
    let builder = match query {
        QueryKind::From => postgrest.from("items").select("*"),
        QueryKind::Rpc => postgrest.rpc("search", r#"{"term":"rust"}"#),
    };
    let request = builder.execute();
    assert!(
        core::mem::size_of_val(&request) <= 1024,
        "PostgREST execution must leave stack space for nested caller futures"
    );
    let request = tokio::spawn(request);

    let error = timeout(Duration::from_secs(5), async {
        accepted_receiver
            .await
            .expect("server stopped before accepting the connection");
        request.await.expect("request task panicked")
    })
    .await
    .expect("request exceeded the test deadline")
    .expect_err("silent server unexpectedly returned a response");

    assert!(matches!(error, rp_postgrest::Error::Request(source) if source.is_timeout()));
    server.abort();
}

#[tokio::test]
async fn supplied_client_timeout_applies_to_from_queries() {
    assert_supplied_client_timeout(QueryKind::From).await;
}

#[tokio::test]
async fn supplied_client_timeout_applies_to_rpc_queries() {
    assert_supplied_client_timeout(QueryKind::Rpc).await;
}
