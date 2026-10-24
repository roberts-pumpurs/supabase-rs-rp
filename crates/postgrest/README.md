# rp-postgrest

[![crates.io](https://img.shields.io/crates/v/rp-postgrest.svg)](https://crates.io/crates/rp-postgrest) [![docs.rs](https://docs.rs/rp-postgrest/badge.svg)](https://docs.rs/rp-postgrest)

A concrete asynchronous PostgREST client owned by this workspace. Requires Rust 1.88 or newer. The package is `rp-postgrest`; the Rust library is `rp_postgrest`.

## Install and configure

```toml
[dependencies]
rp-postgrest = "3.0"
serde = { version = "1", features = ["derive"] }
```

Both constructors are fallible. `Postgrest::new` builds a transport that follows a redirect only to the same origin (scheme, host, and port), up to 10 hops, so the API key does not leak to another host. A supplied Reqwest client keeps its connection pool, default headers, timeout, proxy, TLS policy, and redirect policy. Cloning `Postgrest` shares its immutable configuration and transport.

```rust
use rp_postgrest::{Postgrest, reqwest};
use std::time::Duration;

fn clients() -> Result<(Postgrest, Postgrest), rp_postgrest::Error> {
    let default = Postgrest::new("https://example.supabase.co/rest/v1/")?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(rp_postgrest::ConfigError::Client)?;
    let configured = Postgrest::new_with_client(
        "https://example.supabase.co/rest/v1/", http,
    )?
    .insert_header("apikey", "public-api-key")?
    .auth("access-token")?
    .schema("public");
    Ok((default, configured))
}
```

Bases must be HTTP(S) URLs without credentials, query strings, or fragments. Client header/auth validation is eager. Request-local header/auth, schema validation, and JSON serialization errors are deferred until `build` or execution. `build()` returns `Result<reqwest::RequestBuilder, Error>`, so caller request customization remains available.

The default `rustls` feature enables HTTPS. With `default-features = false`, this crate enables no TLS backend itself. Enable a compatible Reqwest TLS feature in your dependency graph for HTTPS. A supplied client does not add a TLS backend absent from that graph.

## Literal resources and raw grammar

Pass literal SQL resource names to `from`, `rpc`, and `rpc_json`. The client encodes the resource segment once, including `%`, `/`, spaces, Unicode, backslashes, and reserved punctuation. Do not pre-encode names. `from("a#b")` addresses `a%23b`, not a URL fragment. Dot-only resources `.` and `..` return a deferred `Error::Configuration(ConfigError::DotOnlyResource)` because the URL parser normalizes those segments.

Filters and selections are PostgREST grammar, not SQL or pre-encoded URLs. Scalar helpers leave the value literal after the operator. `append_query` appends an unencoded key/value pair without deduplication or interpretation; `query_pairs` exposes the ordered pairs read-only. The HTTP serializer performs URL encoding.

```rust
use rp_postgrest::Postgrest;

fn queries(client: &Postgrest) {
    // Literal list elements: quote and escape in IN context automatically.
    let _safe = client.from("items")
        .in_values("name", ["a,b", "a\"b", "a\\b"]);
    // Raw list fragments: caller supplies PostgREST grammar.
    let _raw = client.from("items")
        .in_("name", ["\"a,b\"", "plain"])
        .or("active.eq.true,name.eq.plain");
}
```

`in_` retains its raw-fragment contract. Use `in_values` for untrusted literal text elements. Neither helper quotes column identifiers. Raw `and`/`or`, containment, range, and full-text helpers require caller grammar. Repeated filters stay ordered; repeated `order` calls compose one effective ordering value.

## Execution and decoding

```rust
use rp_postgrest::{Error, Postgrest};
use serde::Deserialize;

#[derive(Deserialize)]
struct Item { id: i64, name: String }

async fn rows(client: &Postgrest) -> Result<Vec<Item>, Error> {
    client.from("items").select("id,name").fetch::<Vec<Item>>().await
}
```

Choose the response contract explicitly:

- `execute()` returns the raw `reqwest::Response`. It does not check HTTP status or consume the body.
- `execute_checked()` rejects every non-2xx status, including HTTP 300. Successful responses remain unread.
- `fetch::<T>()` checks status and decodes successful JSON into `T` through the central decoder.
- `single()` requests server-enforced single-object cardinality; it does not select the first row locally.

HTTP 204 decodes through Serde's unit deserializer, so `fetch::<()>()` supports void RPC responses. It does not fabricate an empty row array. An empty HTTP 200 body is invalid JSON and returns `Error::ResponseDecode`. Scalar, object, and array RPC responses must match the requested type. `rpc_json` serializes arguments without adding relation `select`, `single`, or representation preferences.

`insert`, `upsert`, `update`, and `rpc` accept raw JSON strings. Their `_json` counterparts serialize typed payloads and retain serialization errors until execution. Inserts, updates, deletes, and upserts request representations by default. Upsert adds `resolution=merge-duplicates`; `on_conflict` selects conflict columns.

## Preferences, counts, and minimal writes

Prefer directives compose by directive key. The last value for a key wins; unrelated directives survive. This includes inherited and request-local `Prefer` headers. `return_minimal()` changes only `return`, and `count(Count)` changes only `count`.

```rust
use rp_postgrest::{Count, Counted, Error, Postgrest};
use serde::Deserialize;

#[derive(Deserialize)]
struct Item { id: i64 }

async fn counted(client: &Postgrest) -> Result<(), Error> {
    let page: Counted<Vec<Item>> = client.from("items")
        .select("id").range(0, 24)
        .fetch_with_count(Count::Exact).await?;
    println!("{} returned, {} total", page.data.len(), page.count);
    let total = client.from("items").eq("active", "true")
        .execute_count(Count::Exact).await?;
    let affected = client.from("items").eq("id", "42")
        .delete().execute_count(Count::Exact).await?;
    println!("{total} matching, {affected} deleted");
    Ok(())
}
```

`Count::Exact`, `Planned`, and `Estimated` select server count modes. `exact_count`, `planned_count`, and `estimated_count` are fluent conveniences. Count preferences do not change pagination. Ranges are inclusive; `limit(0)` requests zero rows.

`fetch_with_count` returns decoded data and the server's `Content-Range` total. `execute_count` turns a GET into HEAD, retaining its filters, profile, and pagination. For mutations it retains the method and requests minimal return. RPC is not changed into a read. Missing, wildcard, or malformed totals return `Error::Count`, never a fabricated zero. `ResponseMetadata::count()` parses an observed total.

To write without decoding rows, use `.return_minimal().execute_checked().await?`. Raw limits and ranges describe response pagination; they are not a guarantee of safely limited UPDATE or DELETE on current servers.

## One error result

All checked and typed operations return one `Result<T, rp_postgrest::Error>`. The non-exhaustive error distinguishes configuration, serialization, request, response-body, structured PostgREST, malformed error-envelope, successful-response decoding, and count failures.

```rust
fn inspect(error: &rp_postgrest::Error) {
    if let Some((status, body)) = error.postgrest_response() {
        eprintln!("PostgREST HTTP {status}: {}", body.message);
    }
    if let Some(metadata) = error.response_metadata() {
        eprintln!("status={}, url={}, headers={:?}",
            metadata.status(), metadata.url(), metadata.headers());
    }
}
```

`postgrest_body()` exposes the canonical decoded code, message, details, and hint. `postgrest_error()` exposes its structured source. HTTP status remains authoritative. PostgREST 16.2 ambiguous embedding errors such as PGRST201 can use HTTP 300 and an array of relationship details; the canonical `ErrorDetails` supports that array as well as ordinary text. Malformed error envelopes retain exact bytes in the `Error::Decode` source. Valid structured errors do not retain the original JSON bytes, and failed body reads do not promise partial bytes. `status()`, `url()`, `response_metadata()`, and the standard error source chain support diagnostics without nested results.

`postgrest_response()` returns the observed HTTP status and a borrowed typed `ErrorResponse` together. It does not allocate or infer status from the error code. It returns `None` for transport failures, malformed error envelopes, and other errors without a decoded server response.

`server_response()` also returns the observed non-success status when the error envelope is malformed or reading the body failed. Its optional body is `None` in those cases. Successful-status decoding and count errors are not server errors. `is_jwt_expired()` requires PGRST301 or PGRST303 and the message `JWT expired`, case-insensitive with an optional final period. Invalid signatures and other JWT failures return `false`.

With the `test-util` feature, `Error::from_response(status, body: &[u8]) -> Option<Error>` decodes fixtures through the production error-envelope decoder. Successful statuses return `None` without decoding. Malformed bytes are copied into the error; valid envelopes retain decoded fields only. Fixture metadata uses empty headers and `http://localhost/`, not an actual request URL.

Enable `serde_json/arbitrary_precision` in the dependency graph when decoding exact PostgreSQL numerics. This avoids an intermediate floating-point conversion; the requested Rust response type still determines its own numeric representation.

## Migration from 2.1

- Import `rp_postgrest`, not the old `postgrest` library name.
- Add `?` to `Postgrest::new`, client `auth`/`insert_header`, and `Builder::build`. Configuration fields are private; use constructors and fluent methods.
- Pass literal resources instead of percent-encoded names. Dot-only names fail explicitly.
- `Builder` is not `Clone`. Start a new query from a cloned/shared `Postgrest` instead of copying mutable request/error state.
- Use `execute` only for deliberate raw status/body handling. Use `execute_checked` or `fetch` for checked responses and handle the flat `Error`; old execute/query error aliases are absent.
- Counts no longer force `Range: 0-0`. Add your desired range or limit explicitly, or use `execute_count` for body-free reads.
- Preserve raw `in_` grammar or switch literal inputs to `in_values`. Do not pre-encode query pairs.
- See [rp-supabase-client](../supabase-client/README.md#migration-from-07) for removal of the product's old response wrapper and nested raw-result decoding.
