#![cfg(all(
    feature = "rest",
    feature = "auth",
    feature = "storage",
    feature = "functions"
))]
#![expect(clippy::unwrap_used, reason = "tests fail loudly on setup errors")]
#![expect(clippy::tests_outside_test_module, reason = "integration test crate")]

use mockito::{Matcher, Mock, ServerGuard};
use supabase_rp::Client;

const LEGACY_KEY: &str = "eyJhbGciOiJIUzI1NiJ9.eyJyb2xlIjoiYW5vbiJ9.c2lnbmF0dXJl";
const PUBLISHABLE_KEY: &str = "sb_publishable_abc123";
const USER_TOKEN: &str = "user-jwt";

async fn expect_all(server: &mut ServerGuard, key: &str, bearer: Matcher) -> Vec<Mock> {
    let mut mocks = Vec::new();
    for (method, path) in [
        ("GET", "/rest/v1/todos"),
        ("GET", "/auth/v1/user"),
        ("GET", "/storage/v1/bucket"),
        ("POST", "/functions/v1/hello"),
        #[cfg(feature = "graphql")]
        ("POST", "/graphql/v1"),
    ] {
        mocks.push(
            server
                .mock(method, Matcher::Regex(format!("^{path}")))
                .match_header("apikey", key)
                .match_header("authorization", bearer.clone())
                .with_status(200)
                .with_body("[]")
                .expect(1)
                .create_async()
                .await,
        );
    }
    mocks
}

async fn call_all(client: &Client) {
    // Only the request headers matter; response decoding is out of scope.
    drop(client.from("todos").select("id").execute().await);
    drop(client.auth().get_user().await);
    drop(client.storage().list_buckets().await);
    drop(client.functions().invoke("hello").send().await);
    #[cfg(feature = "graphql")]
    drop(client.graphql().query("{ __typename }").send().await);
}

/// Every service sends the project key as `apikey`. Before `with_access_token`, the bearer is
/// `anonymous_bearer`; after it, the user token. The original client keeps its bearer rule.
async fn check(key: &str, anonymous_bearer: Matcher) {
    let mut server = mockito::Server::new_async().await;
    let client = Client::new(&format!("{}/", server.url()), key).unwrap();

    let mocks = expect_all(&mut server, key, anonymous_bearer.clone()).await;
    call_all(&client).await;
    for mock in mocks {
        mock.assert_async().await;
        mock.remove_async().await;
    }

    let bearer = Matcher::Exact(format!("Bearer {USER_TOKEN}"));
    let mocks = expect_all(&mut server, key, bearer).await;
    call_all(&client.with_access_token(USER_TOKEN).unwrap()).await;
    for mock in mocks {
        mock.assert_async().await;
        mock.remove_async().await;
    }

    let mocks = expect_all(&mut server, key, anonymous_bearer).await;
    call_all(&client).await;
    for mock in mocks {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn cross_origin_redirect_is_not_followed() {
    let mut origin = mockito::Server::new_async().await;
    let mut other = mockito::Server::new_async().await;
    let target = other
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let redirect = origin
        .mock("GET", "/rest/v1/todos?select=id")
        .with_status(302)
        .with_header("location", &format!("{}/rest/v1/todos", other.url()))
        .expect(1)
        .create_async()
        .await;
    let client = Client::new(&format!("{}/", origin.url()), LEGACY_KEY).unwrap();

    drop(client.from("todos").select("id").execute().await);

    redirect.assert_async().await;
    target.assert_async().await;
}

#[tokio::test]
async fn legacy_key_is_bearer_until_user_token() {
    check(LEGACY_KEY, Matcher::Exact(format!("Bearer {LEGACY_KEY}"))).await;
}

#[tokio::test]
async fn publishable_key_is_never_bearer() {
    check(PUBLISHABLE_KEY, Matcher::Missing).await;
}
