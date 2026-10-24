# rp-postgrest-error

[![crates.io](https://img.shields.io/crates/v/rp-postgrest-error.svg)](https://crates.io/crates/rp-postgrest-error) [![docs.rs](https://docs.rs/rp-postgrest-error/badge.svg)](https://docs.rs/rp-postgrest-error)

Typed error responses for `PostgREST` clients, with exact error codes and malformed-body evidence.

The crate combines the authoritative HTTP status with `PostgREST`'s structured
JSON error body while remaining independent of any HTTP client. It preserves
the exact wire error code and provides typed classifications for `PostgreSQL`,
`PostgREST`, and `PTxyz` custom-status errors. Inferred mappings follow the
[PostgREST v14 error reference](https://docs.postgrest.org/en/v14/references/errors.html).

## Design guarantees

- The status observed on the HTTP response is authoritative and is never
  replaced by a reconstructed value.
- The exact error code is retained even when the crate does not recognize it.
- Future `PostgREST` and `PostgreSQL` codes remain usable without a crate update.
- Unknown PostgREST and custom codes do not receive invented statuses;
  unrecognized valid SQLSTATEs follow PostgREST's documented HTTP 400 fallback.
- Malformed response errors retain the observed status and exact supplied body bytes.
- The crate depends on `http`, `serde`, and `serde_json`, but not `reqwest` or
  an async runtime.

## Installation

```toml
[dependencies]
rp-postgrest-error = "0.8"
```

Requires Rust 1.88 or later.

## Decode an HTTP error response

```rust
use http::StatusCode;
use rp_postgrest_error::{ErrorKind, PostgresErrorCode, PostgrestError};

let body = br#"{
    "code": "23505",
    "message": "duplicate key value violates unique constraint",
    "details": "Key (id)=(1) already exists.",
    "hint": null
}"#;

let error = PostgrestError::from_slice(StatusCode::CONFLICT, body)?;

assert_eq!(error.status(), StatusCode::CONFLICT);
assert_eq!(error.code().as_str(), "23505");
assert_eq!(
    error.kind(),
    ErrorKind::Postgres(PostgresErrorCode::UniqueViolation)
);

# Ok::<(), rp_postgrest_error::DecodeError>(())
```

An HTTP adapter only needs to extract the status and body bytes. For example,
a Reqwest-based client can pass `response.status()` and
`response.bytes().await?.as_ref()` to `PostgrestError::from_slice`. Adapters
that own a `Vec<u8>` can use `PostgrestError::from_vec` so malformed evidence
is retained without another body copy.

`DecodeError::body()` returns the exact bytes supplied to the decoder when the body cannot be decoded as an `ErrorResponse`. Valid structured errors retain their decoded fields, not the original JSON whitespace or byte representation. This crate does not read HTTP streams and cannot preserve bytes an adapter never supplied, including partial bytes lost during a failed response read.

## Typed diagnostic details

`ErrorResponse::details` is `Option<ErrorDetails>`. Ordinary string diagnostics decode as `ErrorDetails::Text(String)`. Ambiguous embedding errors such as PostgREST 16.2's `PGRST201` decode their candidate array as `ErrorDetails::AmbiguousEmbeddings(Vec<EmbeddingDetail>)`. Null or absent details decode as `None`.

Each `EmbeddingDetail` contains `cardinality`, `embedding`, and `relationship`. `EmbeddingCardinality` has `OneToOne`, `OneToMany`, `ManyToOne`, and `ManyToMany` variants, serialized as PostgREST's kebab-case strings. Callers do not need to inspect an untyped JSON value:

```rust
use rp_postgrest_error::{ErrorDetails, PostgrestError};

fn print_candidates(error: &PostgrestError) {
    if let Some(ErrorDetails::AmbiguousEmbeddings(candidates)) =
        &error.response().details
    {
        for candidate in candidates {
            println!("{}: {}", candidate.embedding, candidate.relationship);
        }
    }
}
```

The observed status remains authoritative for either detail shape. For example, an HTTP 300 ambiguous embedding response remains HTTP 300, regardless of any code-derived fallback.

## Migration to 0.8

`ErrorResponse::details` now holds `Option<ErrorDetails>` rather than `Option<String>`. Match `ErrorDetails::Text` for ordinary diagnostics and `ErrorDetails::AmbiguousEmbeddings` for relationship candidates. Constructors, exact error codes, and observed-status access remain unchanged.

## Match `PostgREST` and custom-status errors

```rust
use http::StatusCode;
use rp_postgrest_error::{ErrorCode, ErrorKind, PostgrestErrorCode};

let missing_table = ErrorCode::new("PGRST205");
assert_eq!(
    missing_table.kind(),
    ErrorKind::Postgrest(PostgrestErrorCode::TableNotFound)
);

let payment_required = ErrorCode::new("PT402");
assert_eq!(
    payment_required.kind(),
    ErrorKind::CustomStatus(StatusCode::PAYMENT_REQUIRED)
);
```

`ErrorCode::as_str()` always returns the exact wire value. Known-code enums are
`#[non_exhaustive]`; callers should include a fallback match arm.

`ErrorResponse::sqlstate()` borrows a canonical five-character uppercase ASCII SQLSTATE. It accepts unknown SQLSTATEs and rejects lowercase values, `PGRST` codes, and custom HTTP status codes such as `PT402`.

## Body-only status inference

When no HTTP response is available, `ErrorCode::inferred_status` exposes
code-only mappings as an explicit fallback:

```rust
use http::StatusCode;
use rp_postgrest_error::{Authentication, ErrorCode};

let code = ErrorCode::new("42501");
assert_eq!(
    code.inferred_status(Authentication::Authenticated),
    Some(StatusCode::FORBIDDEN)
);
assert_eq!(
    code.inferred_status(Authentication::Unknown),
    None
);
```

Inference returns `None` for unknown PostgREST/custom codes, mappings that
require the error message, and cases where authentication context is required
but unavailable. `ErrorResponse::inferred_status` additionally handles
PostgREST's message-sensitive PostgreSQL mappings. Prefer
`PostgrestError::status()` whenever an HTTP response exists.

## Migration from 0.3

Version 0.4 deliberately replaces the duplicated wrapper hierarchy:

- `PostgrestUtilError` is replaced by `PostgrestError`.
- Construct errors with `PostgrestError::from_slice(status, body)` or
  `PostgrestError::from_response(status, response)`.
- Match `error.kind()` instead of top-level `Postgres`, `Postgrest`, and
  `Custom` wrapper variants.
- Read shared fields from `error.response()` or consume status and body
  together with `error.into_parts()`.
- Read the exact code with `error.code().as_str()`.
- Use `error.status()` for the observed status.
- Use `error.inferred_status(authentication)` only for body-derived fallback
  behavior.

`ErrorResponse.code` is now an `ErrorCode` rather than a `String`. Its Serde
wire representation remains a JSON string.
