# rp-supabase-functions

[![crates.io](https://img.shields.io/crates/v/rp-supabase-functions.svg)](https://crates.io/crates/rp-supabase-functions) [![docs.rs](https://docs.rs/rp-supabase-functions/badge.svg)](https://docs.rs/rp-supabase-functions)

Client for [Supabase Edge Functions](https://supabase.com/docs/guides/functions).
It calls `{project}/functions/v1/{name}` with your API key, checks the status, and decodes JSON.

## Install

```toml
[dependencies]
rp-supabase-functions = "0.1"
serde = { version = "1", features = ["derive"] }
```

## Quickstart

Create a client with the project URL and the anon key. Invoke a function with a JSON body and
decode the JSON reply.

```rust,no_run
use rp_supabase_functions::FunctionsClient;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Greet<'a> {
    name: &'a str,
}

#[derive(Deserialize)]
struct Reply {
    message: String,
}

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let url = rp_supabase_functions::url::Url::parse("https://abc.supabase.co/")?;
let client = FunctionsClient::new(&url, "anon-key")?;

let reply: Reply = client
    .invoke("hello-world")
    .json(&Greet { name: "Ada" })
    .fetch()
    .await?;
println!("{}", reply.message);
# Ok(())
# }
```

The default method is `POST`. Use `.method(Method::GET)` to change it. Use `.body(bytes, content_type)`
for raw bodies, `.header(name, value)` for extra headers, and `.region("us-east-1")` to pick a region.
Use `.send()` to get the raw `reqwest::Response`, or `.text()` to get the body as a string.

The default HTTP client follows redirects only to the same origin (scheme, host, port), up to 10 hops.
To reuse an existing connection pool, call `FunctionsClient::new_with_client`; that client is used unchanged.

## Call as a signed-in user

Pass the user's access token. The client then sends it in the `Authorization` header.

```rust,no_run
use rp_supabase_functions::{FunctionsClient, Method};

# async fn run(client: FunctionsClient, access_token: &str) -> Result<(), Box<dyn std::error::Error>> {
let user_client = client.with_access_token(access_token)?;
let body = user_client.invoke("me").method(Method::GET).text().await?;
println!("{body}");
# Ok(())
# }
```

## Errors

All methods return `FunctionsError`:

- `Http { status, body }`: the function returned a non-2xx status.
- `Relay { status, body }`: the Supabase relay could not reach the function (`x-relay-error: true`).
- `Decode`: the response body does not match the requested type.
- `Transport`: the request did not complete.
- `InvalidFunctionName`, `HeaderName`, `HeaderValue`, `Serialize`, `Url`, `UrlNotBase`: invalid input.

```rust,no_run
use rp_supabase_functions::{FunctionsClient, FunctionsError};

# async fn run(client: FunctionsClient) {
match client.invoke("hello-world").send().await {
    Ok(response) => println!("status {}", response.status()),
    Err(FunctionsError::Http { status, body }) => eprintln!("function failed with {status}: {body}"),
    Err(other) => eprintln!("{other}"),
}
# }
```

## Rate limits and TLS

`with_retry_policy(RetryPolicy)` sets bounded retries for explicit HTTP 429 responses.
The default replays only GET, HEAD and OPTIONS, never the default POST invocation.
Use mutation replay only when the function rejects a rate-limited request before any side effect.
Transport failures and other HTTP statuses are not replayed by this policy.

The default TLS backend is rustls. For native TLS, disable default features and enable `native-tls`.
rustls takes precedence when both are enabled. Supplied HTTP clients retain their TLS and redirect policies.

## Limits

The client does not stream request bodies. It does not support multipart form helpers; build the
body yourself and pass it to `.body(..)`.

Function names must be non-empty, must not be `.` or `..`, and must not contain `/`, tab, CR, or LF.
Other names fail with `InvalidFunctionName` when you send the request.

The client sends your key in the `apikey` header. It also sends a legacy JWT key as
`Authorization: Bearer <key>`. It does not send new-format keys (`sb_publishable_...`,
`sb_secret_...`) as a bearer token. Call `with_access_token` to send a user token.
