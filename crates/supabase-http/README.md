# rp-supabase-http

Bounded retries for explicit HTTP 429 responses. Shared by the Supabase client crates.

`RetryPolicy::default()` permits two additional attempts for GET, HEAD and OPTIONS.
The exponential delay starts at 200 ms and stops at 30 seconds. `Retry-After`
accepts seconds or an HTTP date. A server delay above the configured maximum
returns the 429 unchanged. The client never shortens that delay.

`RetryPolicy::none()` disables retries. `retry_non_idempotent(true)` permits
replayable mutation bodies. Enable it only when the endpoint rejects a
rate-limited request before performing any side effect.

Transport failures, other status codes, and non-cloneable streaming bodies are
never replayed. The final response keeps its status, headers and body. Dropping
the request future cancels a pending request or backoff. Retries do not provide
exactly-once execution.

Requires Rust 1.88. Default TLS is rustls. For native TLS, disable default features
and enable `native-tls`. A supplied reqwest client keeps its own TLS policy.
