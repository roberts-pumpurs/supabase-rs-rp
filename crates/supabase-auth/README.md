# rp-supabase-auth

[![crates.io](https://img.shields.io/crates/v/rp-supabase-auth.svg)](https://crates.io/crates/rp-supabase-auth) [![docs.rs](https://docs.rs/rp-supabase-auth/badge.svg)](https://docs.rs/rp-supabase-auth)

Typed async client for [Supabase Auth](https://supabase.com/docs/guides/auth) (GoTrue), with a stream that keeps a session's JWT fresh.

```toml
[dependencies]
rp-supabase-auth = "0.11"
```

The client talks to `<project-url>/auth/v1/`. Pass the project base URL, for example `https://abc.supabase.co/`, and the project API key (anon or service role).

## Quickstart

Sign in with a password, read the user, and sign out:

```rust,no_run
use rp_supabase_auth::auth_client::ApiClient;
use rp_supabase_auth::types::LoginCredentials;
use rp_supabase_auth::url;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let url: url::Url = "https://abc.supabase.co/".parse()?;
let api_key = "your-anon-key";

let anonymous = ApiClient::new_unauthenticated(&url, api_key)?;
let credentials = LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());
let session = anonymous.sign_in_with_password(&credentials).await?;

let access_token = session.access_token.ok_or("no access token")?;
let client = ApiClient::new_authenticated(&url, api_key, &access_token)?;
let user = client.get_user().await?;
println!("signed in as {:?}", user.email);

client.sign_out().await?;
# Ok(())
# }
```

`ApiClient` has one method per common flow:

| Method | Endpoint |
| --- | --- |
| `sign_up` | `POST /signup` |
| `sign_in_with_password` | `POST /token?grant_type=password` |
| `refresh_session` | `POST /token?grant_type=refresh_token` |
| `exchange_code_for_session` | `POST /token?grant_type=pkce` |
| `sign_in_with_otp` | `POST /otp` |
| `verify_otp` | `POST /verify` |
| `reset_password_for_email` | `POST /recover` |
| `get_user` | `GET /user` |
| `update_user` | `PUT /user` |
| `sign_out` | `POST /logout` |

`get_user`, `update_user`, and `sign_out` need a client made with `new_authenticated`.

`sign_up` returns a `SignupResponse`. It is `Session` when the user can sign in at once. It is `ConfirmationRequired` with the new user when the project requires email or phone confirmation. No session exists until the user confirms.

## Keep a session fresh

`new_authenticated_stream` signs in, then refreshes the token before it expires. It yields a new authenticated `ApiClient` after each sign-in or refresh. A failed attempt yields an error, and the stream retries up to `max_reconnect_attempts` times. After that, the stream ends.

```rust,no_run
use std::time::Duration;

use rp_supabase_auth::auth_client::new_authenticated_stream;
use rp_supabase_auth::futures::StreamExt as _;
use rp_supabase_auth::jwt_stream::SupabaseAuthConfig;
use rp_supabase_auth::types::LoginCredentials;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let config = SupabaseAuthConfig {
    api_key: "your-anon-key".to_owned(),
    max_reconnect_attempts: 5,
    reconnect_interval: Duration::from_secs(3),
    url: "https://abc.supabase.co/".parse()?,
};
let credentials = LoginCredentials::email("user@example.com".to_owned(), "password".to_owned());

let clients = new_authenticated_stream(config, credentials)?;
let mut clients = std::pin::pin!(clients);
while let Some(item) = clients.next().await {
    match item {
        Ok(client) => println!("user: {:?}", client.get_user().await?.email),
        Err(error) => eprintln!("sign-in or refresh failed: {error}"),
    }
}
# Ok(())
# }
```

To get only the tokens, use `JwtStream::new(config).sign_in(credentials)`. It yields `AccessTokenResponseSchema` values.

## Use your own HTTP client

Each constructor has a `_with_client` variant that takes a `reqwest::Client`. Use it to set timeouts, proxies, TLS policy, default headers, or to share one connection pool. The stream reuses the client for sign-in, every refresh, and every `ApiClient` it yields. The default transport follows a redirect only to the same origin (scheme, host, and port), up to 10 hops; a supplied client keeps its own redirect policy.

```rust,no_run
use std::time::Duration;

use rp_supabase_auth::auth_client::ApiClient;
use rp_supabase_auth::url;

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let http = reqwest::Client::builder()
    .timeout(Duration::from_secs(30))
    .build()?;
let url: url::Url = "https://abc.supabase.co/".parse()?;
let client = ApiClient::new_unauthenticated_with_client(&url, "your-anon-key", http)?;
# Ok(())
# }
```

The example needs a direct `reqwest` dependency, for example `reqwest = { version = "0.13", default-features = false, features = ["rustls"] }`.

The client adds the `apikey`, `Accept`, `Content-Type`, and bearer headers to each request. It does not change the defaults of the client you pass in. The `apikey` and bearer header values are marked sensitive, so they do not appear in debug output.

For PostgREST clients that share this transport, use `anonymous_client_with_client` and `new_authenticated_with_client` from `rp-supabase-client` 0.10. REST errors use the `rp-postgrest` 3.2 error type, not `AuthError`.

## Other endpoints and admin requests

`auth_client::requests` has one type per Auth endpoint, including the admin endpoints. Build a request, then send it with `ApiClient::send`, or use `build_request` for step-by-step control. Admin endpoints need a client made with the service role key.

```rust,no_run
use rp_supabase_auth::auth_client::ApiClient;
use rp_supabase_auth::auth_client::requests::UserGetRequest;

# async fn example(client: ApiClient) -> Result<(), rp_supabase_auth::error::AuthError> {
// One call:
let user = client.send(&UserGetRequest).await?;

// Step by step:
let response = client.build_request(&UserGetRequest)?.execute().await?;
let user = response.json().await?;
# Ok(())
# }
```

Use `Response::ok` instead of `Response::json` for endpoints that return no body.

`TokenRequest` is an enum with one variant per grant: `Password`, `RefreshToken`, `IdToken`, and `Pkce`. The variant sets the `grant_type` query parameter, and each grant body holds only the fields Supabase Auth reads for that grant.

## Errors

Every call returns `Result<_, AuthError>`. A non-success HTTP status becomes `AuthError::Api`. It holds the status and the decoded `ErrorSchema`. `ErrorSchema` reads both the legacy error format and the `2024-01-01` API version format: `error_code` holds the machine-readable code and `msg` holds the message. If the body is not a JSON error object, `ErrorSchema::msg` holds the raw body text.

```rust,no_run
use rp_supabase_auth::auth_client::ApiClient;
use rp_supabase_auth::error::AuthError;
use rp_supabase_auth::types::LoginCredentials;

# async fn example(client: ApiClient, credentials: LoginCredentials) {
match client.sign_in_with_password(&credentials).await {
    Ok(session) => println!("expires in {:?} s", session.expires_in),
    Err(AuthError::Api { status, error }) if status.as_u16() == 400 => {
        println!("rejected: {:?}", error.error_code);
    }
    Err(other) => println!("request failed: {other}"),
}
# }
```

The other variants cover transport failures, invalid URLs, invalid header values, and JSON that does not match the expected type.

## Limits

- `ApiClient::sign_in_with_oauth(AuthorizeRequest)` generates an OS-random verifier and S256 challenge, returning `OAuthPkce { authorization_url, verifier }`. Open the URL yourself, handle the callback, then call `complete_oauth(code, verifier)`. The verifier is deliberately explicit and redacted in Debug.
- `sign_in_anonymously(data, captcha)` uses signup and requires a complete session with `user.is_anonymous == true`.
- Attach `Arc<dyn SessionStore>` using `JwtStream::with_session_store`. `MemorySessionStore` is process-local; optional `file-session-store` enables `FileSessionStore::new(path)`. `with_session` accepts OAuth/anonymous/MFA sessions. `resume().await` loads the stored session and immediately rotates its refresh token without password fallback. Each initial/rotated session is saved before being yielded. Store errors are yielded once and terminate the stream.
- File stores atomically replace the destination entry (including symlinks), create Unix files with mode 0600, and treat missing files as no session and malformed/incomplete files as `SessionStoreError::Corrupt`. Use a private, trusted parent directory: parent-directory symlink attacks, encryption/keychain integration, and multi-process refresh coordination are outside this adapter. Concurrent writers are last-writer-wins; do not refresh the same session concurrently. Failed or cancelled saves make a best-effort synchronous unlink of their temporary file; credential writes remain asynchronous. `clear().await` removes local credentials, not the server session; call logout separately when appropriate.
- `with_retry_policy(RetryPolicy)` configures bounded 429 retries; mutation replay requires explicit opt-in. Supplied HTTP clients keep their transport configuration.

Protocol references: [GoTrue authorization](https://github.com/supabase/auth/blob/master/internal/api/external.go), [auth-js OAuth/anonymous/PKCE](https://github.com/supabase/auth-js/blob/master/src/GoTrueClient.ts), [RFC 7636 S256 vector](https://www.rfc-editor.org/rfc/rfc7636#appendix-B).

Refresh stream state is now private (except its client). Refresh deadlines start at successful token receipt, not the consumer's next poll; retry deadlines start when a failure is yielded, including password sign-in failures. Errors are observable even on the final failed attempt, and failed refreshes never fall back to password login. Dropping the stream cancels its tasks.

## Migration from 0.9

- `LoginCredentials` has no builder. Use `LoginCredentials::email(email, password)` or `LoginCredentials::phone(phone, password)`. The password is required, and `Debug` hides it.
- `TokenRequest` is an enum of grants: `Password(PasswordGrant)`, `RefreshToken(RefreshTokenGrant)`, `IdToken(IdTokenGrant)`, and `Pkce(PkceGrant)`. `TokenRequestBody` and `GrantType` are gone. `PkceGrant` sends `auth_code`, which Supabase Auth reads for the PKCE exchange.
- `SignupPayload` takes `credentials: Option<LoginCredentials>`. Leave it unset to create an anonymous user.
- `SignupPayload`, `OtpRequest`, `RecoverRequest`, and `SsoRequest` take `pkce: Option<PkceChallenge>` instead of separate `code_challenge` and `code_challenge_method` fields. `PkceChallenge::code_challenge_method` is `CodeChallengeMethod::S256` or `CodeChallengeMethod::Plain`.
- `AuthorizeRequest` takes an `OAuthProvider`, `scopes: Vec<String>`, a `Url` for `redirect_to`, and `pkce: Option<PkceChallenge>`. It now sends `code_challenge`.
- `RefreshTokenGrant`, `PkceGrant`, and `IdTokenGrant` hide their tokens in `Debug`.
