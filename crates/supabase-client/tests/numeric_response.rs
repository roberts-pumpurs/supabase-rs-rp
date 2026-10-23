#![expect(
    clippy::tests_outside_test_module,
    reason = "Cargo compiles this integration test as its own test crate."
)]

use rp_supabase_client::rp_postgrest::Postgrest;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Row {
    amount: serde_json::Number,
    id: i64,
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions report a failed numeric precision contract; Result propagates transport setup failures."
)]
#[tokio::test]
async fn numeric_response_preserves_precision_and_integer_width()
-> Result<(), Box<dyn core::error::Error>> {
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/rows")
        .with_header("content-type", "application/json")
        .with_body(r#"[{"amount":123456789012345678901234567890.12345678901234567890,"id":9223372036854775807}]"#)
        .create_async()
        .await;
    let rows: Vec<Row> = Postgrest::new(server.url())?.from("rows").fetch().await?;
    let [row] = rows.as_slice() else {
        return Err("expected exactly one row".into());
    };
    assert_eq!(
        row.amount.to_string(),
        "123456789012345678901234567890.12345678901234567890"
    );
    assert_eq!(row.id, i64::MAX);
    request.assert_async().await;
    Ok(())
}
