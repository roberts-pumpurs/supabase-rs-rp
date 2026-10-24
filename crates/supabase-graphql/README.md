# rp-supabase-graphql

A buffered, typed client for Supabase's `graphql/v1` endpoint.

```rust,no_run
use rp_supabase_graphql::{GraphqlClient, url::Url};
use serde_json::{Value, json};
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = GraphqlClient::new(&Url::parse("https://project.supabase.co")?, "project-key")?;
let response = client.query("query Read($first: Int!) { booksCollection(first: $first) { edges { node { id } } } }")
    .variables(&json!({"first": 10}))
    .operation_name("Read")
    .fetch::<Value>().await?;
// Partial data and execution errors coexist; neither is discarded.
if response.errors.is_empty() {
    println!("{:?}", response.data);
}
# Ok(())
# }
```

`GraphqlClient::{new,new_with_client}` accept a project URL and API key. `with_access_token` clones with a user bearer token. Legacy keys supply both `apikey` and bearer authorization; `sb_publishable_`/`sb_secret_` keys supply only `apikey` until a token is set. All credential and custom header values are marked sensitive. Request debug output omits documents, variables, and custom headers. Treat response/error bodies as potentially sensitive application data.

`query` returns `QueryBuilder`, supporting `variables`, `operation_name`, `header`, and `headers`. Variables are serialized once into buffered raw JSON. `send` returns the raw successful HTTP response; `fetch<T>` decodes a `GraphqlResponse<T>` with optional typed data, execution errors (locations, field/index paths, extensions), and envelope extensions. `GraphqlClientError` distinguishes HTTP, transport, request serialization, response decoding, and malformed empty envelopes. Null data is valid. `into_result` optionally rejects execution errors without losing the partial response.

The default client only follows same-origin redirects. Supplied HTTP clients retain their own redirect/TLS settings: callers must ensure those policies protect credentials. Default TLS is `rustls`; `native-tls` is optional. With both enabled the default constructor prefers rustls.

All operations use POST. Default retry policy therefore sends every request once, including queries. `with_retry_policy(RetryPolicy::new(...).retry_non_idempotent(true))` explicitly permits replay on HTTP 429 only; opt in only when the document is safe to replay. Token clones preserve the policy.

## Local acceptance

The ignored `live` test requires `SUPABASE_URL`, `SUPABASE_ANON_KEY`, and `SUPABASE_SERVICE_ROLE_KEY`; missing variables fail. The parent-managed fixture is `public.graphql_probe(id uuid primary key default gen_random_uuid(), value text not null)`, with GraphQL name inflection enabled, service-role privileges, anonymous SELECT privileges, and RLS enabled without an anonymous policy. It inserts with variables, reads back typed JSON, verifies anonymous RLS filtering, and deletes its own row. Run `cargo test -p rp-supabase-graphql --test live -- --ignored` against that fixture.

Protocol references: [Supabase GraphQL](https://supabase.com/docs/guides/graphql), [pg_graphql API](https://supabase.github.io/pg_graphql/api/), [GraphQL response specification](https://spec.graphql.org/October2021/#sec-Response).
