//! Elevated `GoTrue` administration. Keep service-role credentials on trusted servers only.
use crate::{
    auth_client::{ApiClient, requests::AuthModuleRequest},
    error::AuthError,
    types::{AdminUsersResponse, FactorDeleteResponse, MFAFactorSchema, UserSchema},
};
use core::{fmt, marker::PhantomData};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Optional write attributes; omitted fields are not sent or reset.
#[derive(Clone, Default, Serialize)]
pub struct UserAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_confirm: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_confirm: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_metadata: Option<simd_json::OwnedValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_metadata: Option<simd_json::OwnedValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Go duration, or `none` to remove a ban.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ban_duration: Option<String>,
}
impl fmt::Debug for UserAttributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UserAttributes([REDACTED])")
    }
}

/// Email invitation options.
#[derive(Clone, Serialize)]
pub struct Invite {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<simd_json::OwnedValue>,
}
impl fmt::Debug for Invite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invite([REDACTED])")
    }
}
/// Supported generated email link kinds.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    Signup,
    Invite,
    Magiclink,
    Recovery,
    EmailChangeCurrent,
    EmailChangeNew,
}
/// Generated-link write payload. Redirect destination is passed separately as a query parameter.
#[derive(Clone, Serialize)]
pub struct GenerateLink {
    #[serde(rename = "type")]
    pub link_type: LinkType,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<simd_json::OwnedValue>,
}
impl fmt::Debug for GenerateLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GenerateLink([REDACTED])")
    }
}
/// `GoTrue` returns link properties and user fields in the same object, not an envelope.
#[derive(Clone, Deserialize)]
pub struct GeneratedLink {
    pub action_link: String,
    pub email_otp: Option<String>,
    pub hashed_token: Option<String>,
    pub verification_type: Option<String>,
    pub redirect_to: Option<String>,
    #[serde(flatten)]
    pub user: UserSchema,
}
impl fmt::Debug for GeneratedLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GeneratedLink([REDACTED])")
    }
}
/// Explicit elevated client. No elevation is inferred from a normal user client.
#[derive(Clone)]
pub struct AdminClient {
    inner: ApiClient,
}
impl fmt::Debug for AdminClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdminClient([REDACTED])")
    }
}
#[derive(Debug, Serialize)]
struct DeleteUser {
    should_soft_delete: bool,
}
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct AdminFactors {
    pub factors: Vec<MFAFactorSchema>,
}

impl AdminClient {
    /// Construct using explicit service-role credentials.
    /// # Errors
    /// Returns invalid URL, header or transport configuration errors.
    pub fn new(url: &url::Url, service_role_key: &str) -> Result<Self, AuthError> {
        Ok(Self {
            inner: ApiClient::new_authenticated(url, service_role_key, service_role_key)?,
        })
    }
    /// Construct while preserving the supplied HTTP transport configuration.
    /// # Errors
    /// Returns invalid URL or credential header errors.
    pub fn new_with_client(
        url: &url::Url,
        service_role_key: &str,
        http: reqwest::Client,
    ) -> Result<Self, AuthError> {
        Ok(Self {
            inner: ApiClient::new_authenticated_with_client(
                url,
                service_role_key,
                service_role_key,
                http,
            )?,
        })
    }
    /// Configure the shared retry policy; mutations are not retried by default.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: rp_supabase_http::RetryPolicy) -> Self {
        self.inner = self.inner.with_retry_policy(policy);
        self
    }
    /// List one page of users.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn list_users(
        &self,
        page: Option<u32>,
        per_page: Option<u32>,
    ) -> Result<AdminUsersResponse, AuthError> {
        let mut request = Get::new(&["admin", "users"], ());
        if let Some(page) = page {
            request.query.push(("page", page.to_string()));
        }
        if let Some(per_page) = per_page {
            request.query.push(("per_page", per_page.to_string()));
        }
        self.inner.send(&request).await
    }
    /// Create a user. The response is the direct `GoTrue` user object.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn create_user(&self, attributes: UserAttributes) -> Result<UserSchema, AuthError> {
        self.inner
            .send(&Post::new(&["admin", "users"], attributes))
            .await
    }
    /// Fetch a user by an opaque, unencoded ID.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn get_user(&self, id: &str) -> Result<UserSchema, AuthError> {
        self.inner
            .send(&Get::new(&["admin", "users", id], ()))
            .await
    }
    /// Update only supplied attributes.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn update_user(
        &self,
        id: &str,
        attributes: UserAttributes,
    ) -> Result<UserSchema, AuthError> {
        self.inner
            .send(&Put::new(&["admin", "users", id], attributes))
            .await
    }
    /// Delete a user. Soft deletion is irreversible but retains hashed identification.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn delete_user(
        &self,
        id: &str,
        should_soft_delete: bool,
    ) -> Result<UserSchema, AuthError> {
        self.inner
            .send(&Delete::new(
                &["admin", "users", id],
                DeleteUser { should_soft_delete },
            ))
            .await
    }
    /// Invite a user by email.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn invite(
        &self,
        invite: Invite,
        redirect_to: Option<&str>,
    ) -> Result<UserSchema, AuthError> {
        let mut request = Post::new(&["invite"], invite);
        if let Some(redirect) = redirect_to {
            request.query.push(("redirect_to", redirect.to_owned()));
        }
        self.inner.send(&request).await
    }
    /// Generate a link without sending mail.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn generate_link(
        &self,
        options: GenerateLink,
        redirect_to: Option<&str>,
    ) -> Result<GeneratedLink, AuthError> {
        let mut request = Post::new(&["admin", "generate_link"], options);
        if let Some(redirect) = redirect_to {
            request.query.push(("redirect_to", redirect.to_owned()));
        }
        self.inner.send(&request).await
    }
    /// List a user's MFA factors.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn list_factors(&self, id: &str) -> Result<AdminFactors, AuthError> {
        self.inner
            .send(&Get::new(&["admin", "users", id, "factors"], ()))
            .await
    }
    /// Remove a user's MFA factor.
    /// # Errors
    /// Returns request, server or response decoding errors.
    pub async fn delete_factor(
        &self,
        id: &str,
        factor: &str,
    ) -> Result<FactorDeleteResponse, AuthError> {
        self.inner
            .send(&Delete::new(&["admin", "users", id, "factors", factor], ()))
            .await
    }
}

// All management requests share the core transport and status/error mapping.
macro_rules! request_type {
    ($name:ident, $method:ident) => {
        #[expect(
            clippy::pub_with_shorthand,
            reason = "Rustfmt normalizes crate-private macro declarations to pub(crate)."
        )]
        pub(crate) struct $name<P, R> {
            segments: Vec<String>,
            payload: P,
            query: Vec<(&'static str, String)>,
            result: PhantomData<R>,
        }
        impl<P, R> $name<P, R> {
            pub(crate) fn new(segments: &[&str], payload: P) -> Self {
                Self {
                    segments: segments.iter().map(|value| (*value).to_owned()).collect(),
                    payload,
                    query: Vec::new(),
                    result: PhantomData,
                }
            }
            pub(crate) fn query(&self) -> &[(&'static str, String)] {
                &self.query
            }
        }
        impl<P, R> fmt::Debug for $name<P, R> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        }
        impl<P: Serialize, R: DeserializeOwned + fmt::Debug> AuthModuleRequest for $name<P, R> {
            type Res = R;
            type Payload = P;
            const METHOD: reqwest::Method = reqwest::Method::$method;
            fn path(&self, base: &url::Url) -> Result<url::Url, AuthError> {
                let mut url = base.clone();
                // Core URLs are hierarchical HTTP URLs. Path segments encode each opaque ID once.
                if let Ok(mut path) = url.path_segments_mut() {
                    path.pop_if_empty();
                    for segment in &self.segments {
                        path.push(segment);
                    }
                }
                if !self.query().is_empty() {
                    url.query_pairs_mut().extend_pairs(
                        self.query()
                            .iter()
                            .map(|(key, value)| (*key, value.as_str())),
                    );
                }
                Ok(url)
            }
            fn payload(&self) -> &P {
                &self.payload
            }
        }
    };
}
request_type!(Get, GET);
request_type!(Post, POST);
request_type!(Put, PUT);
request_type!(Delete, DELETE);
