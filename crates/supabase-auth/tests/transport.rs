#![expect(
    clippy::tests_outside_test_module,
    reason = "Integration tests exercise public auth interfaces."
)]
#![expect(
    clippy::unwrap_used,
    reason = "Test setup and assertions retain failure details."
)]

use core::time::Duration;
use futures::StreamExt as _;
use mockito::Matcher;
use rp_supabase_auth::{
    auth_client::{
        ApiClient, new_authenticated_stream_with_client,
        requests::{AuthModuleRequest as _, AuthorizeRequest, TokenRequest},
    },
    jwt_stream::{JwtStream, SupabaseAuthConfig},
    types::{
        CodeChallengeMethod, IdTokenGrant, IdTokenProvider, LoginCredentials, OAuthProvider,
        PasswordGrant, PkceChallenge, PkceGrant, RefreshTokenGrant, SignupPayload,
    },
};

fn http() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-transport",
        reqwest::header::HeaderValue::from_static("retained"),
    );
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_static("Bearer original"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

fn request() -> TokenRequest {
    TokenRequest::Password(PasswordGrant::from(credentials()))
}

fn credentials() -> LoginCredentials {
    LoginCredentials::email("user@example.com".to_owned(), "p".to_owned())
}

fn config(url: &str) -> SupabaseAuthConfig {
    SupabaseAuthConfig {
        api_key: "key".into(),
        url: url::Url::parse(url).unwrap(),
        max_reconnect_attempts: 2,
        reconnect_interval: Duration::from_millis(1),
    }
}

#[tokio::test]
async fn configured_api_clients_keep_headers_and_do_not_mutate_shared_bearer() {
    let mut server = mockito::Server::new_async().await;
    let authenticated = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer user")
        .match_header("accept", "application/json")
        .match_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;
    let unauthenticated = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("apikey", "key")
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer original")
        .with_body("{}")
        .create_async()
        .await;
    let url = url::Url::parse(&server.url()).unwrap();
    let transport = http();
    ApiClient::new_authenticated_with_client(&url, "key", "user", transport.clone())
        .unwrap()
        .build_request(&request())
        .unwrap()
        .execute()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    ApiClient::new_unauthenticated_with_client(&url, "key", transport)
        .unwrap()
        .build_request(&request())
        .unwrap()
        .execute()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    authenticated.assert_async().await;
    unauthenticated.assert_async().await;
}

#[tokio::test]
async fn sign_in_configured_transport_preserves_redirect_policy() {
    let mut server = mockito::Server::new_async().await;
    let login = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("x-transport", "retained")
        .with_status(302)
        .with_header("location", "/unexpected")
        .with_body(r#"{"msg":"redirect rejected"}"#)
        .create_async()
        .await;
    let target = server
        .mock("GET", "/unexpected")
        .with_body(r#"{"access_token":"unexpected-follow"}"#)
        .expect(0)
        .create_async()
        .await;
    let mut stream = JwtStream::new(config(&server.url()))
        .sign_in_with_client(credentials(), http())
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap();
    result.unwrap_err();
    login.assert_async().await;
    target.assert_async().await;
}

#[tokio::test]
async fn authenticated_auth_stream_reuses_transport_after_refresh() {
    let mut server = mockito::Server::new_async().await;
    let login = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer original")
        .with_body(r#"{"access_token":"first","refresh_token":"refresh","expires_in":0}"#)
        .create_async()
        .await;
    let refresh = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded(
            "grant_type".into(),
            "refresh_token".into(),
        ))
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer original")
        .match_body(Matcher::Regex(r#""refresh_token":"refresh""#.into()))
        .with_body(r#"{"access_token":"second","refresh_token":"rotated","expires_in":3600}"#)
        .create_async()
        .await;
    let user_request = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .match_header("x-transport", "retained")
        .match_header("authorization", "Bearer second")
        .match_header("apikey", "key")
        .with_body("{}")
        .create_async()
        .await;
    let stream =
        new_authenticated_stream_with_client(config(&server.url()), credentials(), http()).unwrap();
    futures::pin_mut!(stream);
    let _first = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    second
        .build_request(&request())
        .unwrap()
        .execute()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    login.assert_async().await;
    refresh.assert_async().await;
    user_request.assert_async().await;
}

#[expect(
    clippy::panic,
    reason = "An unexpected auth error fails this timeout contract test"
)]
#[tokio::test]
async fn configured_sign_in_preserves_total_timeout() {
    let mut server = mockito::Server::new_async().await;
    let _slow = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), "password".into()))
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(150));
            writer.write_all(b"{}")
        })
        .create_async()
        .await;
    let transport = reqwest::Client::builder()
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let mut stream = JwtStream::new(config(&server.url()))
        .sign_in_with_client(credentials(), transport)
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    let rp_supabase_auth::jwt_stream::RefreshStreamError::AuthError(
        rp_supabase_auth::error::AuthError::Reqwest(source),
    ) = error
    else {
        panic!("expected configured timeout");
    };
    assert!(source.is_timeout());
}

#[tokio::test]
async fn default_client_does_not_follow_cross_origin_redirect() {
    let mut origin = mockito::Server::new_async().await;
    let mut other = mockito::Server::new_async().await;
    let redirect = origin
        .mock("POST", "/auth/v1/recover")
        .with_status(302)
        .with_header("location", &format!("{}/steal", other.url()))
        .create_async()
        .await;
    let leaked = other.mock("GET", "/steal").expect(0).create_async().await;
    let client =
        ApiClient::new_unauthenticated(&url::Url::parse(&origin.url()).unwrap(), "anon-key")
            .unwrap();

    let result = client.reset_password_for_email("user@example.com").await;

    result.unwrap_err();
    redirect.assert_async().await;
    leaked.assert_async().await;
}

#[tokio::test]
async fn default_client_follows_same_origin_redirect() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("POST", "/auth/v1/recover")
        .with_status(302)
        .with_header("location", "/moved")
        .create_async()
        .await;
    let target = server
        .mock("GET", "/moved")
        .match_header("apikey", "anon-key")
        .with_body("{}")
        .create_async()
        .await;
    let client =
        ApiClient::new_unauthenticated(&url::Url::parse(&server.url()).unwrap(), "anon-key")
            .unwrap();

    client
        .reset_password_for_email("user@example.com")
        .await
        .unwrap();

    redirect.assert_async().await;
    target.assert_async().await;
}

/// Each grant sends its `grant_type` and only the body fields Supabase Auth reads for it.
#[rstest::rstest]
#[case::password(
    TokenRequest::Password(PasswordGrant::from(LoginCredentials::phone("+1".to_owned(), "p".to_owned()))),
    "password",
    r#"{"phone":"+1","password":"p"}"#
)]
#[case::refresh_token(
    TokenRequest::RefreshToken(RefreshTokenGrant { refresh_token: "r".into() }),
    "refresh_token",
    r#"{"refresh_token":"r"}"#
)]
#[case::pkce(
    TokenRequest::Pkce(PkceGrant { auth_code: "c".into(), code_verifier: "v".into() }),
    "pkce",
    r#"{"auth_code":"c","code_verifier":"v"}"#
)]
#[case::id_token(
    TokenRequest::IdToken(
        IdTokenGrant::builder()
            .provider(IdTokenProvider::Custom("corp".into()))
            .id_token("t".into())
            .nonce("n".into())
            .build()
    ),
    "id_token",
    r#"{"provider":"custom:corp","id_token":"t","nonce":"n"}"#
)]
#[tokio::test]
async fn token_grants_send_grant_type_and_exact_body(
    #[case] request: TokenRequest,
    #[case] grant_type: &str,
    #[case] body: &str,
) {
    let mut server = mockito::Server::new_async().await;
    let token = server
        .mock("POST", "/auth/v1/token")
        .match_query(Matcher::UrlEncoded("grant_type".into(), grant_type.into()))
        .match_body(Matcher::JsonString(body.into()))
        .with_body("{}")
        .create_async()
        .await;
    let url = url::Url::parse(&server.url()).unwrap();
    ApiClient::new_unauthenticated(&url, "key")
        .unwrap()
        .send(&request)
        .await
        .unwrap();
    token.assert_async().await;
}

#[tokio::test]
async fn signup_flattens_credentials_and_sends_challenge_method() {
    let mut server = mockito::Server::new_async().await;
    let signup = server
        .mock("POST", "/auth/v1/signup")
        .match_body(Matcher::JsonString(
            r#"{"email":"a@b.c","password":"p","code_challenge":"c","code_challenge_method":"s256"}"#
                .into(),
        ))
        .with_body(r#"{"id":"7d1e5b4c-7f1e-4d0a-9a39-0c2b4c4f2d11"}"#)
        .create_async()
        .await;
    let url = url::Url::parse(&server.url()).unwrap();
    let payload = SignupPayload::builder()
        .credentials(LoginCredentials::email("a@b.c".into(), "p".into()))
        .pkce(PkceChallenge {
            code_challenge: "c".into(),
            code_challenge_method: CodeChallengeMethod::S256,
        })
        .build();
    drop(
        ApiClient::new_unauthenticated(&url, "key")
            .unwrap()
            .sign_up(payload)
            .await,
    );
    signup.assert_async().await;
}

#[test]
fn authorize_url_carries_provider_scopes_and_pkce_challenge() {
    let base = url::Url::parse("https://abc.supabase.co/auth/v1/").unwrap();
    let request = AuthorizeRequest::builder()
        .provider(OAuthProvider::GitHub)
        .scopes(vec!["repo".into(), "read:user".into()])
        .redirect_to(url::Url::parse("http://localhost:3000/callback").unwrap())
        .pkce(PkceChallenge {
            code_challenge: "c".into(),
            code_challenge_method: CodeChallengeMethod::S256,
        })
        .build();
    assert_eq!(
        request.path(&base).unwrap().as_str(),
        "https://abc.supabase.co/auth/v1/authorize?provider=github&scopes=repo+read%3Auser\
         &redirect_to=http%3A%2F%2Flocalhost%3A3000%2Fcallback&code_challenge=c\
         &code_challenge_method=s256"
    );
}
