//! Explicitly selected live acceptance; never silently skips missing configuration.
#![expect(clippy::tests_outside_test_module, reason = "Live integration surface")]
#![expect(
    clippy::unwrap_used,
    reason = "Acceptance assertions retain failure context"
)]
use core::time::Duration;
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit as _, Mac as _};
use rp_supabase_auth::{
    admin::{AdminClient, UserAttributes},
    auth_client::ApiClient,
    mfa::AssuranceLevel,
    types::LoginCredentials,
};
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

#[expect(
    clippy::panic,
    reason = "Explicit live setup fails when configuration is absent"
)]
fn required(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_missing_variable| panic!("live management acceptance requires {name}"))
}

#[expect(
    clippy::big_endian_bytes,
    reason = "RFC 6238 requires network byte order for the counter and truncated HMAC"
)]
#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    clippy::indexing_slicing,
    reason = "RFC 6238 test code uses fixed nonzero divisors and SHA-1's bounded dynamic offset."
)]
async fn totp(secret: &str) -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Avoid submitting a code on the final two seconds of a time step.
    if seconds % 30 >= 28 {
        tokio::time::sleep(Duration::from_secs(31 - seconds % 30)).await;
    }
    let counter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 30;
    let key = BASE32_NOPAD
        .decode(secret.trim_end_matches('=').as_bytes())
        .unwrap();
    let mut mac = Hmac::<Sha1>::new_from_slice(&key).unwrap();
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = u32::from_be_bytes(digest[offset..offset + 4].try_into().unwrap()) & 0x7fff_ffff;
    format!("{:06}", binary % 1_000_000)
}

#[expect(
    clippy::too_many_lines,
    reason = "One live lifecycle retains the shared user and MFA state through cleanup"
)]
#[tokio::test]
#[ignore = "requires an isolated Supabase with anonymous sign-ins and TOTP MFA enabled"]
async fn live_admin_crud_and_anonymous_totp_upgrade() {
    let url = url::Url::parse(&required("SUPABASE_URL")).unwrap();
    let anon_key = required("SUPABASE_ANON_KEY");
    let service_role = required("SUPABASE_SERVICE_ROLE_KEY");
    let admin = AdminClient::new(&url, &service_role).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let email = format!("management-{nonce}@example.com");
    let password = format!("Acceptance-{nonce}-password");
    let created = admin
        .create_user(UserAttributes {
            email: Some(email.clone()),
            password: Some(password.clone()),
            email_confirm: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    let id = created.id.as_deref().unwrap();
    assert_eq!(
        admin.get_user(id).await.unwrap().email.as_deref(),
        Some(email.as_str())
    );
    let new_email = format!("updated-{email}");
    let updated = admin
        .update_user(
            id,
            UserAttributes {
                email: Some(new_email.clone()),
                email_confirm: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.email.as_deref(), Some(new_email.as_str()));
    assert_eq!(
        admin.get_user(id).await.unwrap().email.as_deref(),
        Some(new_email.as_str())
    );
    let page = admin.list_users(Some(1), Some(100)).await.unwrap();
    assert!(page.users.iter().any(|user| user.id.as_deref() == Some(id)));

    let anonymous = ApiClient::new_unauthenticated(&url, &anon_key).unwrap();
    let anonymous_session = anonymous.sign_in_anonymously(None, None).await.unwrap();
    assert_eq!(
        anonymous_session
            .user
            .as_ref()
            .and_then(|user| user.is_anonymous),
        Some(true)
    );
    let anonymous_id = anonymous_session
        .user
        .as_ref()
        .and_then(|user| user.id.as_deref())
        .unwrap()
        .to_owned();
    admin.delete_user(&anonymous_id, false).await.unwrap();
    admin.get_user(&anonymous_id).await.unwrap_err();

    // Anonymous users cannot enroll MFA. Reuse the confirmed normal CRUD user.
    let session = anonymous
        .sign_in_with_password(&LoginCredentials::email(new_email.clone(), password))
        .await
        .unwrap();
    assert_eq!(
        session.user.as_ref().and_then(|user| user.id.as_deref()),
        Some(id)
    );
    let client =
        ApiClient::new_authenticated(&url, &anon_key, session.access_token.as_deref().unwrap())
            .unwrap();
    let initial = client.mfa().assurance_level().await.unwrap();
    assert_eq!(initial.current, AssuranceLevel::Aal1);
    assert_eq!(initial.next, AssuranceLevel::Aal1);
    let enrollment = client
        .mfa()
        .enroll_totp(Some("live-management"), Some("supabase-rs-utils"))
        .await
        .unwrap();
    let factor_id = enrollment.id;
    let secret = enrollment.totp.unwrap().secret;
    assert!(
        client
            .mfa()
            .list_factors()
            .await
            .unwrap()
            .iter()
            .any(|factor| factor.id.as_deref() == Some(factor_id.as_str()))
    );
    let challenge = client.mfa().challenge(&factor_id, None).await.unwrap();
    let code = totp(&secret).await;
    let upgraded_session = client
        .mfa()
        .verify(&factor_id, &challenge.id, &code)
        .await
        .unwrap();
    let upgraded = ApiClient::new_authenticated(
        &url,
        &anon_key,
        upgraded_session.access_token.as_deref().unwrap(),
    )
    .unwrap();
    let assurance = upgraded.mfa().assurance_level().await.unwrap();
    assert_eq!(assurance.current, AssuranceLevel::Aal2);
    assert_eq!(assurance.next, AssuranceLevel::Aal2);
    assert!(
        admin
            .list_factors(id)
            .await
            .unwrap()
            .factors
            .iter()
            .any(|factor| factor.id.as_deref() == Some(factor_id.as_str()))
    );
    upgraded.mfa().unenroll(&factor_id).await.unwrap();
    assert!(
        !upgraded
            .mfa()
            .list_factors()
            .await
            .unwrap()
            .iter()
            .any(|factor| factor.id.as_deref() == Some(factor_id.as_str()))
    );
    admin.delete_user(id, false).await.unwrap();
    admin.get_user(id).await.unwrap_err();
}
