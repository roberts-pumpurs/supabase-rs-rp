# rp-supabase-storage

[![crates.io](https://img.shields.io/crates/v/rp-supabase-storage.svg)](https://crates.io/crates/rp-supabase-storage) [![docs.rs](https://docs.rs/rp-supabase-storage/badge.svg)](https://docs.rs/rp-supabase-storage)

Async Rust client for the [Supabase Storage](https://supabase.com/docs/guides/storage) API.
It manages buckets, uploads and downloads objects, and creates signed and public URLs.

## Install

```toml
[dependencies]
rp-supabase-storage = "0.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Quickstart

```rust,no_run
use rp_supabase_storage::url::Url;
use rp_supabase_storage::{BucketOptions, FileOptions, ListOptions, StorageClient};

# async fn run() -> Result<(), rp_supabase_storage::StorageError> {
let project = Url::parse("https://abc.supabase.co/")?;
let storage = StorageClient::new(&project, "service-role-key")?;

// Create a private bucket that accepts PNG images up to 1 MiB.
let options = BucketOptions {
    public: false,
    file_size_limit: Some(1024 * 1024),
    allowed_mime_types: Some(vec!["image/png".to_owned()]),
};
storage.create_bucket("avatars", &options).await?;

let avatars = storage.from("avatars");

// Upload an object.
let file = FileOptions {
    content_type: Some("image/png".to_owned()),
    cache_control: 3600,
    upsert: true,
};
avatars.upload("users/1.png", vec![0_u8; 16], &file).await?;

// Share it for 60 seconds.
let link = avatars.create_signed_url("users/1.png", 60).await?;
println!("{link}");

// Download, list, and remove.
let bytes = avatars.download("users/1.png").await?;
let entries = avatars.list("users", &ListOptions::default()).await?;
avatars.remove(&["users/1.png"]).await?;
# drop((bytes, entries));
# Ok(())
# }
```

Use `StorageClient::new_with_client` to reuse an existing `reqwest::Client` and its connection pool.
The default client from `StorageClient::new` follows redirects (up to 10) only to the same origin, so
the API key never goes to another host; a client you pass in is used unchanged.

## Auth model

Every request sends the `apikey` header. The `Authorization` header depends on the key and token:

- `StorageClient::new` with a legacy JWT key (anon or service role) also sends
  `Authorization: Bearer <api_key>`. With the service role key, the client bypasses row level
  security (RLS). Use it only on a server.
- `StorageClient::new` with a new-format key (`sb_publishable_...` or `sb_secret_...`) sends no
  `Authorization` header. These keys are not JWTs; the gateway checks the `apikey` header.
- `with_access_token(user_jwt)` returns a client that sends the user's JWT as the bearer token.
  Storage then applies the RLS policies on `storage.objects` and `storage.buckets` for that user.
  Use the anon key plus a user token in code that acts for a user.

```rust,no_run
# use rp_supabase_storage::{url::Url, StorageClient};
# fn run(user_jwt: &str) -> Result<(), rp_supabase_storage::StorageError> {
let storage = StorageClient::new(&Url::parse("https://abc.supabase.co/")?, "anon-key")?;
let as_user = storage.with_access_token(user_jwt)?;
# drop(as_user);
# Ok(())
# }
```

`public_url` sends no request. The URL works only when the bucket is public. For private
buckets, call `create_signed_url` or `create_signed_urls`. Both use the batch signing endpoint,
because the single-object endpoint returns an invalid signature for keys with `?`. The API
returns signed URLs relative to `/storage/v1` with the object key unescaped; this crate rebuilds
each URL from the bucket and the encoded object path, then appends the token. Keys with `?` and
spaces work. The Storage server itself rejects some characters in keys, for example `#`.

`BucketOptions` fields are always sent. On `update_bucket`, `None` removes an existing file size
limit or MIME type list. `FileOptions::default()` sends `Cache-Control: max-age=3600`.

## Object paths

Separate path segments with `/`, for example `users/1.png`. The client percent-encodes each
segment once. It rejects empty paths, empty segments (`a//b`, leading or trailing `/`), `.` and
`..`, tabs, carriage returns, and line feeds with `StorageError::InvalidPath` before it sends a
request.

## Errors

All operations return `StorageError`:

- `Api { status, body }`: the server answered with a non-success status. `body` is
  `ApiErrorBody::Storage` with optional `status_code`, `code`, `error`, and `message` when the
  server sent the standard Storage error JSON, else `ApiErrorBody::Raw` with the body text.
- `InvalidPath`: a bucket id or object path is not valid.
- `MissingSignedToken`: a signed URL from the server has no `?token=` query.
- `SignFailed { path, message }`: `create_signed_url` got no URL for the object, for example
  because it does not exist or the caller cannot read it.
- `Http`, `Json`, `Url`, `InvalidHeader`, `InvalidBaseUrl`: transport, decoding, and input errors.

```rust,no_run
# use rp_supabase_storage::{FileOptions, StorageClient, StorageError, StorageErrorCode};
# async fn run(storage: StorageClient) -> Result<(), StorageError> {
match storage.from("blobs").upload("a.bin", vec![1_u8], &FileOptions::default()).await {
    Ok(_) => {}
    Err(error) => match error.code() {
        Some(StorageErrorCode::KeyAlreadyExists | StorageErrorCode::ResourceAlreadyExists) => {
            println!("already uploaded");
        }
        _ => return Err(error),
    },
}
# Ok(())
# }
```

`StorageError::code()` returns the body's machine-readable `code` as a `StorageErrorCode`. A code
this crate does not know is `StorageErrorCode::Other`.

Most object errors arrive as HTTP `400` with the real status in the body's `statusCode`.
`StorageError::api_status()` returns that status, or the HTTP status when the body has none.

`Bucket::remove` is the bulk endpoint. It skips missing paths and paths that row level security
hides, without an error. `Bucket::remove_object` deletes one object and fails when nothing is
deleted: `api_status()` is `404` and `code()` is `NoSuchKey` for a missing object, and
`api_status()` is `403` and `code()` is `AccessDenied` for a denied delete.

`Bucket::download` reads the whole body into memory. `Bucket::download_stream` returns after the
headers arrive. Read the body with `chunk()` or `into_stream()`, and stop when the byte count
passes your limit:

```rust,no_run
# use rp_supabase_storage::{Bucket, StorageError};
# async fn run(bucket: Bucket<'_>, limit: usize) -> Result<Option<Vec<u8>>, StorageError> {
let mut download = bucket.download_stream("blobs/1.bin").await?;
let mut body = Vec::new();
while let Some(chunk) = download.chunk().await? {
    if body.len() + chunk.len() > limit {
        return Ok(None);
    }
    body.extend_from_slice(&chunk);
}
# Ok(Some(body))
# }
```

## Images and resumable uploads

`TransformOptions` supports width/height, `ResizeMode::{Cover, Contain, Fill}`, quality and
`ImageFormat::Origin`. Use `public_transformed_url`, `download_public_transformed`,
`download_transformed` (authenticated streaming), or `create_signed_transformed_url`.
Image transformation must be enabled on the server. Signing submits the options in the
`transform` body and reconstructs the trusted local `render/image/sign` URL from the returned
token. Ordinary signing retains the batch workaround for object names containing `?`.

Signed transformations validate the returned JWT's typed `url` claim against the exact
requested bucket/key before returning a URL (this is not cryptographic verification).
Known upstream limitation: the single-object signing route uses JavaScript `decodeURI`,
which preserves reserved escapes such as `%3F`; a question-mark key can therefore produce
a claim that the image renderer rejects with `InvalidSignature`. The client reports
`StorageError::Protocol` with `remote signed-path mismatch` instead of issuing an unusable
URL. It cannot repair a signed claim without the server's signing key, and never uploads
a transformed replacement object. See upstream
[`getSignedURL.ts`](https://github.com/supabase/storage/blob/master/src/http/routes/object/getSignedURL.ts)
and [`signObjectUrl`](https://github.com/supabase/storage/blob/master/src/storage/object.ts).

`create_resumable_upload(path, length, options)` creates a TUS 1.0 session.
Persist its sensitive `url()` **and** the creation option `upsert`; reopen it using
`StorageClient::resume_upload(&url, upsert)`. The overwrite intent is sent on POST, HEAD,
and every PATCH, including after reopening.
Creation completes a zero-byte object; an empty chunk on a confirmed complete upload is a local no-op.
`write_chunk(offset, Bytes)` accepts exactly 6 MiB except the final tail, including zero bytes
for an empty object. No full-file buffering is required. Reopening performs HEAD; conflicts
and mismatched acknowledgements reconcile with HEAD. A failed or cancelled PATCH leaves
the session dirty, and the next write consults HEAD before accepting bytes. Supply bytes
from the refreshed offset; stale offsets are rejected rather than silently replayed.
Expired uploads return the server's 404/410 error and must be recreated explicitly.

`with_retry_policy` configures shared 429 handling and survives token/project clones.
PATCH is always sent once, irrespective of mutation-retry opt-in.
Default clients follow redirects only within the same origin. A supplied HTTP client retains
its configuration; callers **must** configure no redirects or same-origin-only redirects,
because arbitrary redirect policies can forward the custom `apikey` header.

Protocol references:
[Supabase image transformations](https://supabase.com/docs/guides/storage/serving/image-transformations),
[Supabase resumable uploads](https://supabase.com/docs/guides/storage/uploads/resumable-uploads),
[TUS 1.0 protocol](https://tus.io/protocols/resumable-upload).

The ignored `live_transforms_and_resumable_roundtrip` test requires `SUPABASE_URL` and
`SUPABASE_SERVICE_ROLE_KEY`, an image-enabled local server, and permission to create buckets.
It checks exact 64×64-to-16×16 PNG rendering and resumed upload/overwrite larger than 6 MiB
byte-for-byte. `regression_live_questionmark_signed_transform_reports_remote_mismatch`
checks the explicit upstream signing error for question-mark keys.

## Limits

- No S3 protocol support, analytics buckets, or vector buckets.
