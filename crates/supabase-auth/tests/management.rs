#![expect(
    clippy::tests_outside_test_module,
    reason = "Public integration surface"
)]
#![expect(clippy::unwrap_used, reason = "Test setup and assertions")]
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use mockito::Matcher;
use rp_supabase_auth::{
    admin::{AdminClient, GenerateLink, Invite, LinkType, UserAttributes},
    auth_client::ApiClient,
    mfa::{AssuranceLevel, MfaError},
};
const USER: &str = r#"{"id":"u","aud":"authenticated","role":"authenticated","email":"user@example.com","phone":"","app_metadata":{},"user_metadata":{},"created_at":"2026-10-01T12:00:00Z","updated_at":"2026-10-01T12:00:00Z"}"#;
fn jwt(aal: &str) -> String {
    format!(
        "e30.{}.c2ln",
        URL_SAFE_NO_PAD.encode(format!(r#"{{"aal":"{aal}"}}"#))
    )
}

#[tokio::test]
async fn admin_crud_uses_elevated_headers_and_write_only_fields() {
    let mut server = mockito::Server::new_async().await;
    let client = AdminClient::new_with_client(
        &url::Url::parse(&server.url()).unwrap(),
        "service-secret",
        reqwest::Client::new(),
    )
    .unwrap();
    assert!(!format!("{client:?}").contains("service-secret"));
    let attributes = UserAttributes {
        password: Some("password-secret".into()),
        email_confirm: Some(true),
        ..Default::default()
    };
    assert!(!format!("{attributes:?}").contains("password-secret"));
    let create = server
        .mock("POST", "/auth/v1/admin/users")
        .match_header("authorization", "Bearer service-secret")
        .match_header("apikey", "service-secret")
        .match_body(Matcher::JsonString(
            r#"{"password":"password-secret","email_confirm":true}"#.into(),
        ))
        .with_status(200)
        .with_body(USER)
        .create_async()
        .await;
    assert_eq!(
        client.create_user(attributes).await.unwrap().id.as_deref(),
        Some("u")
    );
    create.assert_async().await;
    let get = server
        .mock("GET", "/auth/v1/admin/users/a%2Fb%25%3F")
        .with_body(USER)
        .create_async()
        .await;
    client.get_user("a/b%?").await.unwrap();
    get.assert_async().await;
    let update = server
        .mock("PUT", "/auth/v1/admin/users/u")
        .match_body(Matcher::JsonString("{}".into()))
        .with_body(USER)
        .create_async()
        .await;
    client
        .update_user("u", UserAttributes::default())
        .await
        .unwrap();
    update.assert_async().await;
    let delete = server
        .mock("DELETE", "/auth/v1/admin/users/u")
        .match_body(Matcher::JsonString(r#"{"should_soft_delete":true}"#.into()))
        .with_body(USER)
        .create_async()
        .await;
    client.delete_user("u", true).await.unwrap();
    delete.assert_async().await;
    let list = server
        .mock("GET", "/auth/v1/admin/users")
        .match_query(Matcher::UrlEncoded("page".into(), "2".into()))
        .with_body(format!(r#"{{"users":[{USER}]}}"#))
        .create_async()
        .await;
    assert_eq!(
        client.list_users(Some(2), None).await.unwrap().users.len(),
        1
    );
    list.assert_async().await;
}

#[tokio::test]
async fn admin_invite_links_factors_and_server_failures() {
    let mut server = mockito::Server::new_async().await;
    let client =
        AdminClient::new(&url::Url::parse(&server.url()).unwrap(), "service-secret").unwrap();
    let invite = server
        .mock("POST", "/auth/v1/invite")
        .match_body(Matcher::JsonString(
            r#"{"email":"user@example.com"}"#.into(),
        ))
        .match_query(Matcher::UrlEncoded(
            "redirect_to".into(),
            "https://app.test/callback?next=/".into(),
        ))
        .with_body(USER)
        .create_async()
        .await;
    client
        .invite(
            Invite {
                email: "user@example.com".into(),
                data: None,
            },
            Some("https://app.test/callback?next=/"),
        )
        .await
        .unwrap();
    invite.assert_async().await;
    let link_body = format!(
        r#"{{"action_link":"https://secret-link","email_otp":"secret-otp",{}}}"#,
        USER.strip_prefix('{').unwrap().strip_suffix('}').unwrap()
    );
    let link = server
        .mock("POST", "/auth/v1/admin/generate_link")
        .match_body(Matcher::JsonString(
            r#"{"type":"recovery","email":"user@example.com"}"#.into(),
        ))
        .with_body(link_body)
        .create_async()
        .await;
    let generated = client
        .generate_link(
            GenerateLink {
                link_type: LinkType::Recovery,
                email: "user@example.com".into(),
                password: None,
                new_email: None,
                data: None,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(generated.user.id.as_deref(), Some("u"));
    assert_eq!(generated.action_link, "https://secret-link");
    assert!(!format!("{generated:?}").contains("secret"));
    link.assert_async().await;
    let factors = server
        .mock("GET", "/auth/v1/admin/users/u/factors")
        .with_body(r#"[{"id":"f","status":"verified","factor_type":"totp"}]"#)
        .create_async()
        .await;
    assert_eq!(client.list_factors("u").await.unwrap().factors.len(), 1);
    factors.assert_async().await;
    let remove = server
        .mock("DELETE", "/auth/v1/admin/users/u/factors/f")
        .with_body(r#"{"id":"f"}"#)
        .create_async()
        .await;
    assert_eq!(client.delete_factor("u", "f").await.unwrap().id, "f");
    remove.assert_async().await;
    let fail = server
        .mock("GET", "/auth/v1/admin/users/missing")
        .with_status(403)
        .with_body(r#"{"msg":"not allowed"}"#)
        .create_async()
        .await;
    let error = client.get_user("missing").await.unwrap_err();
    assert!(!format!("{error:?}").contains("service-secret"));
    fail.assert_async().await;
}

#[tokio::test]
async fn totp_lifecycle_returns_session_and_requires_explicit_token_upgrade() {
    let mut server = mockito::Server::new_async().await;
    let url = url::Url::parse(&server.url()).unwrap();
    let old_token = jwt("aal1");
    let new_token = jwt("aal2");
    let client = ApiClient::new_authenticated(&url, "anon", &old_token).unwrap();
    let enroll = server.mock("POST", "/auth/v1/factors").match_body(Matcher::JsonString(r#"{"factor_type":"totp","friendly_name":"test","issuer":"app"}"#.into())).with_body(r#"{"id":"f","type":"totp","totp":{"qr_code":"qr-secret","secret":"totp-secret","uri":"otpauth://secret"}}"#).create_async().await;
    let enrollment = client
        .mfa()
        .enroll_totp(Some("test"), Some("app"))
        .await
        .unwrap();
    assert_eq!(enrollment.totp.as_ref().unwrap().secret, "totp-secret");
    assert!(!format!("{enrollment:?}").contains("totp-secret"));
    enroll.assert_async().await;
    let challenge = server
        .mock("POST", "/auth/v1/factors/f%2Fid/challenge")
        .match_body(Matcher::JsonString("{}".into()))
        .with_body(r#"{"id":"c","type":"totp","expires_at":2000000000}"#)
        .create_async()
        .await;
    assert_eq!(client.mfa().challenge("f/id", None).await.unwrap().id, "c");
    challenge.assert_async().await;
    let verify = server.mock("POST", "/auth/v1/factors/f/verify").match_header("authorization", format!("Bearer {old_token}").as_str()).match_body(Matcher::JsonString(r#"{"challenge_id":"c","code":"123456"}"#.into())).with_body(format!(r#"{{"access_token":"{new_token}","refresh_token":"refresh","token_type":"bearer","expires_in":3600,"user":{USER}}}"#)).create_async().await;
    let session = client.mfa().verify("f", "c", "123456").await.unwrap();
    verify.assert_async().await;
    let user_body = format!(
        r#"{{{},"factors":[{{"id":"f","status":"verified","factor_type":"totp"}}]}}"#,
        USER.strip_prefix('{').unwrap().strip_suffix('}').unwrap()
    );
    let old = server
        .mock("GET", "/auth/v1/user")
        .match_header("authorization", format!("Bearer {old_token}").as_str())
        .with_body(&user_body)
        .create_async()
        .await;
    let state = client.mfa().assurance_level().await.unwrap();
    assert_eq!(state.current, AssuranceLevel::Aal1);
    assert_eq!(state.next, AssuranceLevel::Aal2);
    old.assert_async().await;
    let upgraded =
        ApiClient::new_authenticated(&url, "anon", session.access_token.as_deref().unwrap())
            .unwrap();
    let new = server
        .mock("GET", "/auth/v1/user")
        .match_header("authorization", format!("Bearer {new_token}").as_str())
        .with_body(user_body)
        .create_async()
        .await;
    assert_eq!(
        upgraded.mfa().assurance_level().await.unwrap().current,
        AssuranceLevel::Aal2
    );
    new.assert_async().await;
    let unenroll = server
        .mock("DELETE", "/auth/v1/factors/f")
        .with_body(r#"{"id":"f"}"#)
        .create_async()
        .await;
    assert_eq!(upgraded.mfa().unenroll("f").await.unwrap().id, "f");
    unenroll.assert_async().await;
}

#[tokio::test]
async fn regression_mfa_verify_rejects_incomplete_sessions() {
    let mut server = mockito::Server::new_async().await;
    let client =
        ApiClient::new_authenticated(&server.url().parse().unwrap(), "anon", &jwt("aal1")).unwrap();
    for body in [
        "{}",
        r#"{"access_token":"upgraded","refresh_token":"refresh"}"#,
        r#"{"access_token":"upgraded","expires_in":3600}"#,
    ] {
        let verify = server
            .mock("POST", "/auth/v1/factors/f/verify")
            .match_body(Matcher::JsonString(
                r#"{"challenge_id":"c","code":"123456"}"#.into(),
            ))
            .with_status(200)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let result = client.mfa().verify("f", "c", "123456").await;
        assert!(
            matches!(
                result,
                Err(rp_supabase_auth::error::AuthError::InvalidSession(_))
            ),
            "MFA verification must reject incomplete session response: {body}"
        );
        verify.assert_async().await;
        verify.remove_async().await;
    }
}

#[tokio::test]
async fn malformed_assurance_claims_are_typed_and_do_not_make_requests() {
    let server = mockito::Server::new_async().await;
    let url = url::Url::parse(&server.url()).unwrap();
    for token in [
        "not-a-jwt",
        "e30.!!!!.sig",
        "e30.e30.sig",
        "e30.eyJhYWwiOiJhYWwzIn0.sig",
    ] {
        let client = ApiClient::new_authenticated(&url, "anon", token).unwrap();
        assert!(matches!(
            client.mfa().assurance_level().await,
            Err(MfaError::MalformedClaims)
        ));
    }
    let client = ApiClient::new_unauthenticated(&url, "anon").unwrap();
    assert!(matches!(
        client.mfa().assurance_level().await,
        Err(MfaError::MissingBearer)
    ));
}

#[tokio::test]
async fn phone_options_and_aal1_without_verified_factors() {
    let mut server = mockito::Server::new_async().await;
    let client = ApiClient::new_authenticated(
        &url::Url::parse(&server.url()).unwrap(),
        "anon",
        &jwt("aal1"),
    )
    .unwrap();
    let user = server
        .mock("GET", "/auth/v1/user")
        .with_body(USER)
        .create_async()
        .await;
    let assurance = client.mfa().assurance_level().await.unwrap();
    assert_eq!(assurance.current, AssuranceLevel::Aal1);
    assert_eq!(assurance.next, AssuranceLevel::Aal1);
    user.assert_async().await;
    let enroll = server
        .mock("POST", "/auth/v1/factors")
        .match_body(Matcher::JsonString(
            r#"{"factor_type":"phone","phone":"+15555550123"}"#.into(),
        ))
        .with_body(r#"{"id":"p","type":"phone","phone":"+15555550123"}"#)
        .create_async()
        .await;
    assert_eq!(
        client
            .mfa()
            .enroll_phone("+15555550123", None)
            .await
            .unwrap()
            .phone
            .as_deref(),
        Some("+15555550123")
    );
    enroll.assert_async().await;
    let challenge = server
        .mock("POST", "/auth/v1/factors/p/challenge")
        .match_body(Matcher::JsonString(r#"{"channel":"sms"}"#.into()))
        .with_body(r#"{"id":"c","type":"phone","expires_at":2000000000}"#)
        .create_async()
        .await;
    client
        .mfa()
        .challenge("p", Some(rp_supabase_auth::mfa::PhoneChannel::Sms))
        .await
        .unwrap();
    challenge.assert_async().await;
}
