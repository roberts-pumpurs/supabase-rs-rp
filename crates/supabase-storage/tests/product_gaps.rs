#![expect(clippy::tests_outside_test_module, reason = "Integration tests")]
#![expect(
    clippy::unwrap_used,
    clippy::default_numeric_fallback,
    reason = "Test fixtures"
)]
use bytes::Bytes;
use mockito::{Matcher, Server};
use rp_supabase_storage::{
    FileOptions, ImageFormat, ResizeMode, StorageClient, TUS_CHUNK_SIZE, TransformOptions, url::Url,
};

// Structurally valid JWT fixture only; no cryptographic verification is implied.
fn signed_token(path: &str) -> String {
    use base64::Engine as _;
    let claims = serde_json::json!({"url": path});
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).unwrap());
    format!("eyJhbGciOiJIUzI1NiJ9.{payload}.fixture")
}

#[tokio::test]
async fn transformed_reserved_paths_and_signed_options() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let bucket = client.from("images");
    let options = TransformOptions {
        width: Some(42),
        height: Some(24),
        resize: Some(ResizeMode::Contain),
        quality: Some(80),
        format: Some(ImageFormat::Origin),
    };
    let public = bucket
        .public_transformed_url("a ?#/x.png", &options)
        .unwrap();
    assert!(public.path().ends_with("a%20%3F%23/x.png"));
    assert!(
        public
            .query_pairs()
            .any(|(key, value)| key == "width" && value == "42")
    );
    // This mock checks URL construction only, not server-side signature validity.
    let sign = server.mock("POST", "/storage/v1/object/sign/images/a%20%3F%23/x.png")
        .match_body(Matcher::Json(serde_json::json!({"expiresIn":60,"transform":{"width":42,"height":24,"resize":"contain","quality":80,"format":"origin"}})))
        .with_body(serde_json::json!({"signedURL": format!("https://evil.invalid/arbitrary?token={}", signed_token("images/a ?#/x.png"))}).to_string()).create_async().await;
    let signed = bucket
        .create_signed_transformed_url("a ?#/x.png", 60, &options)
        .await
        .unwrap();
    assert_eq!(signed.origin(), public.origin());
    assert!(signed.path().starts_with("/storage/v1/render/image/sign/"));
    sign.assert_async().await;
    for route in ["public", "authenticated"] {
        let mock = server
            .mock(
                "GET",
                format!("/storage/v1/render/image/{route}/images/x.png").as_str(),
            )
            .match_query(Matcher::Any)
            .match_header("authorization", "Bearer key")
            .with_body("image")
            .create_async()
            .await;
        let mut download = if route == "public" {
            bucket
                .download_public_transformed("x.png", &options)
                .await
                .unwrap()
        } else {
            bucket
                .download_transformed("x.png", &options)
                .await
                .unwrap()
        };
        assert_eq!(download.chunk().await.unwrap().unwrap(), "image");
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn signed_transform_claims_match_exact_reserved_object_names() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let bucket = client.from("images");
    // JWT fixtures check claims, not cryptographic signatures. Never double-decode names.
    for (requested, claimed, expected_match) in [
        ("q?.png", "images/q?.png", true),
        ("q?.png", "images/q%3F.png", false),
        ("q%3F.png", "images/q%3F.png", true),
        ("q%3F.png", "images/q?.png", false),
        ("q%.png", "images/q%.png", true),
        ("q%25.png", "images/q%25.png", true),
        ("q%25.png", "images/q%.png", false),
        ("normal.png", "another/normal.png", false),
    ] {
        let endpoint = bucket
            .public_transformed_url(requested, &TransformOptions::default())
            .unwrap()
            .path()
            .replace("/render/image/public/", "/object/sign/");
        let mock = server
            .mock("POST", endpoint.as_str())
            .with_body(
                serde_json::json!({
                    "signedURL": format!("/ignored?token={}", signed_token(claimed))
                })
                .to_string(),
            )
            .create_async()
            .await;
        let result = bucket
            .create_signed_transformed_url(requested, 60, &TransformOptions::default())
            .await;
        if expected_match {
            result.unwrap();
        } else {
            assert!(matches!(
                result,
                Err(rp_supabase_storage::StorageError::Protocol(message))
                    if message.contains("remote signed-path mismatch")
            ));
        }
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn signed_transform_preserves_server_errors_and_rejects_invalid_claims() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let bucket = client.from("images");
    let denied = server
        .mock("POST", "/storage/v1/object/sign/images/missing.png")
        .with_status(403)
        .with_body(r#"{"message":"permission denied"}"#)
        .create_async()
        .await;
    assert!(matches!(
        bucket.create_signed_transformed_url("missing.png", 60, &TransformOptions::default()).await,
        Err(rp_supabase_storage::StorageError::Api { status, .. })
            if status == reqwest::StatusCode::FORBIDDEN
    ));
    denied.assert_async().await;
    let malformed = server
        .mock("POST", "/storage/v1/object/sign/images/x.png")
        .with_body(r#"{"signedURL":"/ignored?token=invalid"}"#)
        .create_async()
        .await;
    assert!(matches!(
        bucket
            .create_signed_transformed_url("x.png", 60, &TransformOptions::default())
            .await,
        Err(rp_supabase_storage::StorageError::Protocol(
            "signed image response has invalid token claims"
        ))
    ));
    malformed.assert_async().await;
}

#[tokio::test]
async fn tus_creation_metadata_chunks_conflict_resume_and_expiration() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let chunk_length = u64::try_from(TUS_CHUNK_SIZE).unwrap();
    let upload_length = chunk_length.checked_add(1).unwrap();
    let create = server.mock("POST", "/storage/v1/upload/resumable").match_header("tus-resumable", "1.0.0")
        .match_header("authorization", "Bearer key").match_header("upload-length", upload_length.to_string().as_str())
        .match_header("upload-metadata", "bucketName Yg==,objectName eA==,contentType YXBwbGljYXRpb24vb2N0ZXQtc3RyZWFt,cacheControl MzYwMA==")
        .with_status(201).with_header("tus-resumable", "1.0.0").with_header("location", "/storage/v1/upload/resumable/id").create_async().await;
    let mut upload = client
        .from("b")
        .create_resumable_upload("x", upload_length, &FileOptions::default())
        .await
        .unwrap();
    create.assert_async().await;
    assert!(matches!(
        upload.write_chunk(0, Bytes::from_static(b"x")).await,
        Err(rp_supabase_storage::StorageError::Protocol(
            "TUS chunk must be 6 MiB except the final tail"
        ))
    ));
    let patch = server
        .mock("PATCH", "/storage/v1/upload/resumable/id")
        .match_header("upload-offset", "0")
        .match_header("content-type", "application/offset+octet-stream")
        .with_status(409)
        .create_async()
        .await;
    let head = server
        .mock("HEAD", "/storage/v1/upload/resumable/id")
        .with_status(200)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", chunk_length.to_string().as_str())
        .with_header("upload-length", upload_length.to_string().as_str())
        .expect(2)
        .create_async()
        .await;
    assert!(matches!(
        upload
            .write_chunk(0, Bytes::from(vec![7; TUS_CHUNK_SIZE]))
            .await,
        Err(rp_supabase_storage::StorageError::Protocol(
            "TUS offset conflict; offset refreshed"
        ))
    ));
    assert_eq!(upload.offset(), chunk_length);
    patch.assert_async().await;
    let url = upload.url().clone();
    let mut resumed = client.resume_upload(&url, false).await.unwrap();
    head.assert_async().await;
    assert!(matches!(
        resumed.write_chunk(0, Bytes::from_static(b"x")).await,
        Err(rp_supabase_storage::StorageError::Protocol(
            "stale TUS chunk offset"
        ))
    ));
    let tail = server
        .mock("PATCH", "/storage/v1/upload/resumable/id")
        .match_header("upload-offset", chunk_length.to_string().as_str())
        .with_status(204)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", upload_length.to_string().as_str())
        .create_async()
        .await;
    assert_eq!(
        resumed
            .write_chunk(chunk_length, Bytes::from_static(b"x"))
            .await
            .unwrap(),
        upload_length
    );
    tail.assert_async().await;
    let expired = server
        .mock("HEAD", "/storage/v1/upload/resumable/expired")
        .with_status(410)
        .create_async()
        .await;
    assert!(matches!(
        client
            .resume_upload(
                &Url::parse(&format!(
                    "{}/storage/v1/upload/resumable/expired",
                    server.url()
                ))
                .unwrap(),
                false,
            )
            .await,
        Err(rp_supabase_storage::StorageError::Api { status, .. })
            if status == reqwest::StatusCode::GONE
    ));
    expired.assert_async().await;
}

#[tokio::test]
async fn tus_rejects_unsafe_locations_and_supports_empty_upload() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    for location in [
        "https://evil.invalid/upload",
        "http://user@localhost/upload",
        "/upload#fragment",
    ] {
        let mock = server
            .mock("POST", "/storage/v1/upload/resumable")
            .with_status(201)
            .with_header("tus-resumable", "1.0.0")
            .with_header("location", location)
            .create_async()
            .await;
        assert!(matches!(
            client
                .from("b")
                .create_resumable_upload("x", 0, &FileOptions::default())
                .await,
            Err(rp_supabase_storage::StorageError::Protocol(
                "unsafe resumable upload URL"
            ))
        ));
        mock.assert_async().await;
        mock.remove_async().await;
    }
    let create = server
        .mock("POST", "/storage/v1/upload/resumable")
        .with_status(201)
        .with_header("tus-resumable", "1.0.0")
        .with_header("location", "/empty")
        .create_async()
        .await;
    let mut upload = client
        .from("b")
        .create_resumable_upload("x", 0, &FileOptions::default())
        .await
        .unwrap();
    let patch = server
        .mock("PATCH", "/empty")
        .match_header("upload-offset", "0")
        .with_status(409)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", "0")
        .expect(0)
        .create_async()
        .await;
    assert_eq!(upload.write_chunk(0, Bytes::new()).await.unwrap(), 0);
    create.assert_async().await;
    patch.assert_async().await;
}

#[expect(
    clippy::expect_used,
    reason = "Ignored live tests require explicit environment configuration"
)]
fn live_client() -> StorageClient {
    let url = std::env::var("SUPABASE_URL").expect("SUPABASE_URL required");
    let key =
        std::env::var("SUPABASE_SERVICE_ROLE_KEY").expect("SUPABASE_SERVICE_ROLE_KEY required");
    StorageClient::new(&Url::parse(&url).unwrap(), &key).unwrap()
}

#[tokio::test]
#[ignore = "Requires local Supabase; set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY"]
#[expect(
    clippy::too_many_lines,
    reason = "One isolated bucket lifecycle covers rendering, resumed upload, and resumed overwrite"
)]
async fn live_transforms_and_resumable_roundtrip() {
    use base64::Engine as _;
    let client = live_client();
    let id = format!("storage-gap-{}", std::process::id());
    client
        .create_bucket(
            &id,
            &rp_supabase_storage::BucketOptions {
                public: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let bucket = client.from(&id);
    let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAeklEQVR4nO3PUQkAIBTAwFfGnkYzmiH8OITBAtxm7fN1wwUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWPHYBC+BRS+h/JDcAAAAASUVORK5CYII=").unwrap();
    bucket
        .upload(
            "pixel.png",
            png,
            &FileOptions {
                content_type: Some("image/png".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let transform = TransformOptions {
        width: Some(16),
        height: Some(16),
        resize: Some(ResizeMode::Fill),
        format: Some(ImageFormat::Origin),
        ..Default::default()
    };
    let mut public = bucket
        .download_public_transformed("pixel.png", &transform)
        .await
        .unwrap();
    let mut authenticated = bucket
        .download_transformed("pixel.png", &transform)
        .await
        .unwrap();
    let signed = bucket
        .create_signed_transformed_url("pixel.png", 60, &transform)
        .await
        .unwrap();
    let signed_bytes = reqwest::get(signed)
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let mut public_bytes = Vec::new();
    while let Some(chunk) = public.chunk().await.unwrap() {
        public_bytes.extend_from_slice(&chunk);
    }
    let mut authenticated_bytes = Vec::new();
    while let Some(chunk) = authenticated.chunk().await.unwrap() {
        authenticated_bytes.extend_from_slice(&chunk);
    }
    for bytes in [
        public_bytes.as_slice(),
        authenticated_bytes.as_slice(),
        signed_bytes.as_ref(),
    ] {
        assert_eq!(bytes.get(..8), Some(b"\x89PNG\r\n\x1a\n".as_slice()));
        assert_eq!(
            bytes.get(16..24),
            Some([0_u8, 0, 0, 16, 0, 0, 0, 16].as_slice())
        );
    }
    let mut upload = bucket
        .create_resumable_upload(
            "large.bin",
            u64::try_from(TUS_CHUNK_SIZE + 3).unwrap(),
            &FileOptions::default(),
        )
        .await
        .unwrap();
    upload
        .write_chunk(0, Bytes::from(vec![37; TUS_CHUNK_SIZE]))
        .await
        .unwrap();
    let handle = upload.url().clone();
    let mut resumed = client.resume_upload(&handle, false).await.unwrap();
    resumed
        .write_chunk(resumed.offset(), Bytes::from_static(b"end"))
        .await
        .unwrap();
    let downloaded = bucket.download("large.bin").await.unwrap();
    assert_eq!(downloaded.len(), TUS_CHUNK_SIZE + 3);
    assert!(
        downloaded
            .get(..TUS_CHUNK_SIZE)
            .unwrap()
            .iter()
            .all(|byte| *byte == 37)
    );
    assert_eq!(downloaded.get(TUS_CHUNK_SIZE..).unwrap(), b"end");
    let new_bytes = vec![91; TUS_CHUNK_SIZE + 3];
    let (first_chunk, final_chunk) = new_bytes.split_at(TUS_CHUNK_SIZE);
    let mut overwrite = bucket
        .create_resumable_upload(
            "large.bin",
            u64::try_from(new_bytes.len()).unwrap(),
            &FileOptions {
                upsert: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    overwrite
        .write_chunk(0, Bytes::copy_from_slice(first_chunk))
        .await
        .unwrap();
    // Persist and explicitly restore the session's overwrite intent.
    let mut reopened = client.resume_upload(overwrite.url(), true).await.unwrap();
    reopened
        .write_chunk(reopened.offset(), Bytes::copy_from_slice(final_chunk))
        .await
        .unwrap();
    assert_eq!(bucket.download("large.bin").await.unwrap(), new_bytes);
    let mut empty = bucket
        .create_resumable_upload("empty.bin", 0, &FileOptions::default())
        .await
        .unwrap();
    assert_eq!(empty.write_chunk(0, Bytes::new()).await.unwrap(), 0);
    assert_eq!(bucket.download("empty.bin").await.unwrap(), Bytes::new());
    client.empty_bucket(&id).await.unwrap();
    client.delete_bucket(&id).await.unwrap();
}

#[tokio::test]
async fn cancelled_patch_requires_head_before_accepting_more_bytes() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let create = server
        .mock("POST", "/storage/v1/upload/resumable")
        .with_status(201)
        .with_header("tus-resumable", "1.0.0")
        .with_header("location", "/cancelled")
        .create_async()
        .await;
    let mut upload = client
        .from("b")
        .create_resumable_upload("x", 1, &FileOptions::default())
        .await
        .unwrap();
    create.assert_async().await;
    {
        let future = upload.write_chunk(0, Bytes::from_static(b"x"));
        futures_util::pin_mut!(future);
        assert!(futures_util::poll!(future).is_pending());
    }
    assert!(upload.needs_refresh());
    let head = server
        .mock("HEAD", "/cancelled")
        .with_status(200)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", "1")
        .with_header("upload-length", "1")
        .create_async()
        .await;
    assert!(matches!(
        upload.write_chunk(0, Bytes::from_static(b"x")).await,
        Err(rp_supabase_storage::StorageError::Protocol(
            "stale TUS chunk offset"
        ))
    ));
    assert_eq!(upload.offset(), 1);
    head.assert_async().await;
}

#[tokio::test]
async fn mismatched_patch_offset_reconciles_server_state() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let create = server
        .mock("POST", "/storage/v1/upload/resumable")
        .with_status(201)
        .with_header("tus-resumable", "1.0.0")
        .with_header("location", "/mismatch")
        .create_async()
        .await;
    let mut upload = client
        .from("b")
        .create_resumable_upload("x", 1, &FileOptions::default())
        .await
        .unwrap();
    let patch = server
        .mock("PATCH", "/mismatch")
        .with_status(204)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", "0")
        .create_async()
        .await;
    let head = server
        .mock("HEAD", "/mismatch")
        .with_status(200)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", "1")
        .with_header("upload-length", "1")
        .create_async()
        .await;
    assert!(matches!(
        upload.write_chunk(0, Bytes::from_static(b"x")).await,
        Err(rp_supabase_storage::StorageError::Protocol(
            "TUS PATCH offset mismatch; offset refreshed"
        ))
    ));
    assert_eq!(upload.offset(), 1);
    assert!(!upload.needs_refresh());
    create.assert_async().await;
    patch.assert_async().await;
    head.assert_async().await;
}

#[tokio::test]
async fn regression_resumable_upsert_is_sent_on_patch() {
    let mut server = Server::new_async().await;
    let client =
        StorageClient::new(&Url::parse(&format!("{}/", server.url())).unwrap(), "key").unwrap();
    let create = server
        .mock("POST", "/storage/v1/upload/resumable")
        .match_header("x-upsert", "true")
        .with_status(201)
        .with_header("tus-resumable", "1.0.0")
        .with_header("location", "/upsert")
        .create_async()
        .await;
    let patch = server
        .mock("PATCH", "/upsert")
        .match_header("x-upsert", "true")
        .match_header("upload-offset", "0")
        .match_body("new")
        .with_status(204)
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", "3")
        .create_async()
        .await;
    let head = server
        .mock("HEAD", "/upsert")
        .match_header("x-upsert", "true")
        .with_header("tus-resumable", "1.0.0")
        .with_header("upload-offset", "0")
        .with_header("upload-length", "3")
        .expect(2)
        .create_async()
        .await;
    let mut upload = client
        .from("b")
        .create_resumable_upload(
            "existing.bin",
            3,
            &FileOptions {
                upsert: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    upload.refresh().await.unwrap();
    let mut upload = client.resume_upload(upload.url(), true).await.unwrap();
    head.assert_async().await;
    assert_eq!(
        upload
            .write_chunk(0, Bytes::from_static(b"new"))
            .await
            .unwrap(),
        3
    );
    assert_eq!(upload.offset(), 3);
    create.assert_async().await;
    patch.assert_async().await;
}

#[tokio::test]
#[ignore = "Requires local Supabase image renderer; set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY"]
async fn regression_live_questionmark_signed_transform_reports_remote_mismatch() {
    use base64::Engine as _;
    let client = live_client();
    let id = format!("storage-questionmark-{}", std::process::id());
    client
        .create_bucket(&id, &rp_supabase_storage::BucketOptions::default())
        .await
        .unwrap();
    let bucket = client.from(&id);
    let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAeklEQVR4nO3PUQkAIBTAwFfGnkYzmiH8OITBAtxm7fN1wwUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWPHYBC+BRS+h/JDcAAAAASUVORK5CYII=").unwrap();
    bucket
        .upload(
            "question?.png",
            png,
            &FileOptions {
                content_type: Some("image/png".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let result = bucket
        .create_signed_transformed_url(
            "question?.png",
            60,
            &TransformOptions {
                width: Some(16),
                height: Some(16),
                format: Some(ImageFormat::Origin),
                ..Default::default()
            },
        )
        .await;
    client.empty_bucket(&id).await.unwrap();
    client.delete_bucket(&id).await.unwrap();
    // Upstream signObjectUrl uses decodeURI, leaving %3F in the signed claim.
    // Rewriting the URL cannot repair that claim without the server's signing key.
    assert!(matches!(
        result,
        Err(rp_supabase_storage::StorageError::Protocol(message))
            if message.contains("remote signed-path mismatch")
    ));
}
