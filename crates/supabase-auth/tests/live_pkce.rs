//! Exercise the real PKCE grant without external OAuth provider credentials.
#![expect(
    clippy::tests_outside_test_module,
    reason = "Live integration test crate"
)]
#![expect(
    clippy::unwrap_used,
    reason = "Selected acceptance tests fail with protocol context"
)]
#![expect(clippy::panic, reason = "Missing live prerequisites fail explicitly")]

use core::time::Duration;
use rp_supabase_auth::{
    admin::{AdminClient, UserAttributes},
    auth_client::{
        ApiClient,
        requests::{AuthorizeRequest, OtpRequest},
    },
    error::AuthError,
    types::OAuthProvider,
};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("live PKCE acceptance requires {name}"))
}

#[derive(Deserialize)]
struct MailIndex {
    messages: Vec<MailSummary>,
}
#[derive(Deserialize)]
struct MailSummary {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "To")]
    to: Vec<Recipient>,
}
#[derive(Deserialize)]
struct Recipient {
    #[serde(rename = "Address")]
    address: String,
}
#[derive(Deserialize)]
struct Mail {
    #[serde(rename = "Text")]
    text: String,
    #[serde(rename = "HTML")]
    html: String,
}

fn verification_url(body: &str) -> Option<Url> {
    body.split("http").skip(1).find_map(|tail| {
        let end = tail
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '<' | '>' | ')' | ']')
            })
            .unwrap_or(tail.len());
        let candidate = format!("http{}", tail.get(..end)?).replace("&amp;", "&");
        Url::parse(&candidate)
            .ok()
            .filter(|url| url.path() == "/auth/v1/verify")
    })
}

async fn wait_for_link(http: &reqwest::Client, mailpit: &str, email: &str) -> Url {
    for _attempt in 0_u8..50 {
        let index: MailIndex = http
            .get(format!("{mailpit}/api/v1/messages"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(summary) = index.messages.iter().find(|message| {
            message
                .to
                .iter()
                .any(|recipient| recipient.address == email)
        }) {
            let mail: Mail = http
                .get(format!("{mailpit}/api/v1/message/{}", summary.id))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            if let Some(url) = verification_url(&mail.text).or_else(|| verification_url(&mail.html))
            {
                return url;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("Mailpit did not deliver the PKCE verification link");
}

#[tokio::test]
#[ignore = "requires local Supabase and SUPABASE_MAILPIT_URL"]
async fn email_callback_exchanges_pkce_and_rejects_code_reuse() {
    let url = Url::parse(&required("SUPABASE_URL")).unwrap();
    let key = required("SUPABASE_ANON_KEY");
    let admin = AdminClient::new(&url, &required("SUPABASE_SERVICE_ROLE_KEY")).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let email = format!("pkce-{nonce}@example.com");
    let user = admin
        .create_user(UserAttributes {
            email: Some(email.clone()),
            email_confirm: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    let api = ApiClient::new_unauthenticated(&url, &key).unwrap();
    let flow = api
        .sign_in_with_oauth(
            AuthorizeRequest::builder()
                .provider(OAuthProvider::GitHub)
                .build(),
        )
        .unwrap();
    let challenge = flow.verifier.challenge();
    api.sign_in_with_otp(
        &OtpRequest::builder()
            .email(email.clone())
            .create_user(false)
            .pkce(challenge)
            .build(),
    )
    .await
    .unwrap();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let verify = wait_for_link(&http, &required("SUPABASE_MAILPIT_URL"), &email).await;
    assert_eq!(verify.origin(), url.origin());
    let redirect_response = http.get(verify).send().await.unwrap();
    assert!(redirect_response.status().is_redirection());
    let callback = Url::parse(
        redirect_response.headers()[reqwest::header::LOCATION]
            .to_str()
            .unwrap(),
    )
    .unwrap();
    let code = callback
        .query_pairs()
        .find_map(|(name, value)| (name == "code").then(|| value.into_owned()))
        .unwrap();
    let verifier = flow.verifier.as_str().to_owned();
    let session = api.complete_oauth(&code, flow.verifier).await.unwrap();
    assert_eq!(
        session.user.as_ref().and_then(|entry| entry.id.as_deref()),
        user.id.as_deref()
    );
    let signed_in =
        ApiClient::new_authenticated(&url, &key, session.access_token.as_deref().unwrap()).unwrap();
    assert_eq!(signed_in.get_user().await.unwrap().id, user.id);
    let reused = api
        .exchange_code_for_session(&code, &verifier)
        .await
        .unwrap_err();
    assert!(matches!(reused, AuthError::Api { status, .. } if status.is_client_error()));
    admin
        .delete_user(user.id.as_deref().unwrap(), false)
        .await
        .unwrap();
}
