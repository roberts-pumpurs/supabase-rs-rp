use reqwest::Method;
use serde::Serialize;
use url::Url;

use crate::error::AuthError;
use crate::types;

pub trait AuthModuleRequest {
    /// The successful result type to be returned
    type Res: serde::de::DeserializeOwned + core::fmt::Debug;
    /// The payload that we will send as JSON during the request body
    type Payload: serde::Serialize;

    /// The HTTP method to use
    const METHOD: reqwest::Method;

    /// Returns the path for the request.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be joined or is invalid.
    fn path(&self, base_url: &Url) -> Result<Url, AuthError>;
    /// The payload to send in the request body
    fn payload(&self) -> &Self::Payload;
}

/// Health Check Request
#[derive(Debug, Clone)]
pub struct HealthCheckRequest;

impl AuthModuleRequest for HealthCheckRequest {
    type Res = String;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("health").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Request to `POST /token`. The variant sets the `grant_type` query parameter and the body.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum TokenRequest {
    /// Sign in with email or phone and a password.
    Password(types::PasswordGrant),
    /// Exchange a refresh token for a new session.
    RefreshToken(types::RefreshTokenGrant),
    /// Sign in with an OIDC ID token.
    IdToken(types::IdTokenGrant),
    /// Exchange a PKCE auth code for a session.
    Pkce(types::PkceGrant),
}

impl TokenRequest {
    const fn grant_type(&self) -> &'static str {
        match self {
            Self::Password(_) => "password",
            Self::RefreshToken(_) => "refresh_token",
            Self::IdToken(_) => "id_token",
            Self::Pkce(_) => "pkce",
        }
    }
}

impl AuthModuleRequest for TokenRequest {
    type Res = types::AccessTokenResponseSchema;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("token").map_err(AuthError::from)?;
        url.query_pairs_mut()
            .append_pair("grant_type", self.grant_type());
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Logout Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
#[builder(field_defaults(default, setter(strip_option)))]
pub struct LogoutRequest {
    pub scope: Option<String>,
}

impl AuthModuleRequest for LogoutRequest {
    type Res = ();
    type Payload = ();

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("logout").map_err(AuthError::from)?;
        if let Some(ref scope) = self.scope {
            url.query_pairs_mut().append_pair("scope", scope);
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Verify GET Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct VerifyGetRequest {
    pub token: String,
    pub verification_type: String,
    pub redirect_to: Option<String>,
}

impl AuthModuleRequest for VerifyGetRequest {
    type Res = ();
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("verify").map_err(AuthError::from)?;
        url.query_pairs_mut()
            .append_pair("token", &self.token)
            .append_pair("type", &self.verification_type);
        if let Some(ref redirect_to) = self.redirect_to {
            url.query_pairs_mut()
                .append_pair("redirect_to", redirect_to);
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Verify POST Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
#[builder(field_defaults(default, setter(strip_option)))]
pub struct VerifyPostRequest {
    #[serde(rename = "type")]
    #[builder(!default, setter(!strip_option))]
    pub verification_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gotrue_meta_security: Option<types::GoTrueMetaSecurity>,
}

impl AuthModuleRequest for VerifyPostRequest {
    type Res = types::AccessTokenResponseSchema;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("verify").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Start of an OAuth sign-in (`GET /authorize`).
///
/// Send the user's browser to the URL from [`AuthModuleRequest::path`]. Supabase redirects to
/// the provider and then to `redirect_to`. With a PKCE challenge, the redirect carries a `code`
/// for [`ApiClient::exchange_code_for_session`](crate::auth_client::ApiClient::exchange_code_for_session).
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
#[builder(field_defaults(default, setter(strip_option)))]
pub struct AuthorizeRequest {
    #[builder(!default, setter(!strip_option))]
    pub provider: types::OAuthProvider,
    /// Extra provider scopes, sent space-separated.
    #[builder(setter(!strip_option))]
    pub scopes: Vec<String>,
    pub invite_token: Option<String>,
    pub redirect_to: Option<Url>,
    /// Set it to get a `code` instead of tokens in the redirect.
    pub pkce: Option<types::PkceChallenge>,
}

impl AuthModuleRequest for AuthorizeRequest {
    type Res = ();
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("authorize").map_err(AuthError::from)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("provider", &self.provider.to_string());
            if !self.scopes.is_empty() {
                query.append_pair("scopes", &self.scopes.join(" "));
            }
            if let Some(invite_token) = &self.invite_token {
                query.append_pair("invite_token", invite_token);
            }
            if let Some(redirect_to) = &self.redirect_to {
                query.append_pair("redirect_to", redirect_to.as_str());
            }
            if let Some(pkce) = &self.pkce {
                query
                    .append_pair("code_challenge", &pkce.code_challenge)
                    .append_pair("code_challenge_method", pkce.code_challenge_method.as_str());
            }
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Signup Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct SignupRequest {
    pub payload: types::SignupPayload,
}

impl AuthModuleRequest for SignupRequest {
    type Res = types::SignupResponse;
    type Payload = types::SignupPayload;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("signup").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &self.payload
    }
}

/// Recover Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
#[builder(field_defaults(default, setter(strip_option)))]
pub struct RecoverRequest {
    #[builder(!default, setter(!strip_option))]
    pub email: String,
    #[serde(flatten)]
    pub pkce: Option<types::PkceChallenge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gotrue_meta_security: Option<types::GoTrueMetaSecurity>,
}

impl AuthModuleRequest for RecoverRequest {
    type Res = ();
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("recover").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Resend Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct ResendRequest {
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(rename = "type")]
    pub resend_type: String,
    pub gotrue_meta_security: Option<types::GoTrueMetaSecurity>,
}

impl AuthModuleRequest for ResendRequest {
    type Res = types::ResendResponse;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("resend").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Magic Link Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct MagicLinkRequest {
    pub email: String,
    pub data: Option<types::UserMetadata>,
    pub gotrue_meta_security: Option<types::GoTrueMetaSecurity>,
}

impl AuthModuleRequest for MagicLinkRequest {
    type Res = ();
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("magiclink").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// OTP Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
#[builder(field_defaults(default, setter(strip_option)))]
pub struct OtpRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<types::UserMetadata>,
    #[serde(flatten)]
    pub pkce: Option<types::PkceChallenge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gotrue_meta_security: Option<types::GoTrueMetaSecurity>,
}

impl AuthModuleRequest for OtpRequest {
    type Res = types::OtpResponse;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("otp").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// User GET Request
#[derive(Debug, Clone)]
pub struct UserGetRequest;

impl AuthModuleRequest for UserGetRequest {
    type Res = types::UserSchema;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("user").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// User PUT Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
#[builder(field_defaults(default, setter(strip_option)))]
pub struct UserUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<types::UserMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_metadata: Option<types::AppMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

impl AuthModuleRequest for UserUpdateRequest {
    type Res = types::UserSchema;
    type Payload = Self;

    const METHOD: Method = Method::PUT;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("user").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Reauthenticate Request
#[derive(Debug, Clone)]
pub struct ReauthenticateRequest;

impl AuthModuleRequest for ReauthenticateRequest {
    type Res = ();
    type Payload = ();

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("reauthenticate").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Factors Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct FactorsRequest {
    pub factor_type: String,
    pub friendly_name: Option<String>,
    pub issuer: Option<String>,
    pub phone: Option<String>,
}

impl AuthModuleRequest for FactorsRequest {
    type Res = types::FactorsResponse;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("factors").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Factors Challenge Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct FactorsChallengeRequest {
    pub factor_id: String,
    pub channel: Option<String>,
}

impl AuthModuleRequest for FactorsChallengeRequest {
    type Res = types::ChallengeResponse;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("factors/{}/challenge", self.factor_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Factors Verify Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct FactorsVerifyRequest {
    pub factor_id: String,
    pub challenge_id: String,
    pub code: String,
}

impl AuthModuleRequest for FactorsVerifyRequest {
    type Res = types::AccessTokenResponseSchema;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("factors/{}/verify", self.factor_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Factors Delete Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct FactorsDeleteRequest {
    pub factor_id: String,
}

impl AuthModuleRequest for FactorsDeleteRequest {
    type Res = types::FactorDeleteResponse;
    type Payload = ();

    const METHOD: Method = Method::DELETE;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("factors/{}", self.factor_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Callback GET Request
#[derive(Debug, Clone)]
pub struct CallbackGetRequest;

impl AuthModuleRequest for CallbackGetRequest {
    type Res = ();
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("callback").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Callback POST Request
#[derive(Debug, Clone)]
pub struct CallbackPostRequest;

impl AuthModuleRequest for CallbackPostRequest {
    type Res = ();
    type Payload = ();

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("callback").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// SSO Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct SsoRequest {
    pub domain: Option<String>,
    pub provider_id: Option<String>,
    pub redirect_to: Option<String>,
    pub skip_http_redirect: Option<bool>,
    #[serde(flatten)]
    pub pkce: Option<types::PkceChallenge>,
    pub gotrue_meta_security: Option<types::GoTrueMetaSecurity>,
}

impl AuthModuleRequest for SsoRequest {
    type Res = types::SsoResponse;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("sso").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

// Additional requests for other endpoints can be created following the same pattern.

/// SAML Metadata Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct SamlMetadataRequest {
    pub download: Option<bool>,
}

impl AuthModuleRequest for SamlMetadataRequest {
    type Res = String; // The response is XML content as a string
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("saml/metadata").map_err(AuthError::from)?;
        if let Some(download) = self.download {
            url.query_pairs_mut()
                .append_pair("download", &download.to_string());
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// SAML ACS Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct SamlAcsRequest {
    pub relay_state: Option<String>,
    pub saml_response: Option<String>,
    pub saml_art: Option<String>,
}

impl AuthModuleRequest for SamlAcsRequest {
    type Res = (); // The response is a redirect
    type Payload = ();

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("saml/acs").map_err(AuthError::from)?;
        if let Some(ref relay_state) = self.relay_state {
            url.query_pairs_mut().append_pair("RelayState", relay_state);
        }
        if let Some(ref saml_response) = self.saml_response {
            url.query_pairs_mut()
                .append_pair("SAMLResponse", saml_response);
        }
        if let Some(ref saml_art) = self.saml_art {
            url.query_pairs_mut().append_pair("SAMLArt", saml_art);
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Invite Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct InviteRequest {
    pub email: String,
    pub data: Option<types::UserMetadata>,
}

impl AuthModuleRequest for InviteRequest {
    type Res = types::UserSchema;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("invite").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Admin Generate Link Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct AdminGenerateLinkRequest {
    #[serde(rename = "type")]
    pub link_type: String,
    pub email: String,
    pub new_email: Option<String>,
    pub password: Option<String>,
    pub data: Option<types::UserMetadata>,
    pub redirect_to: Option<String>,
}

impl AuthModuleRequest for AdminGenerateLinkRequest {
    type Res = types::AdminGenerateLinkResponse;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url
            .join("admin/generate_link")
            .map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Admin Audit Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminAuditRequest {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

impl AuthModuleRequest for AdminAuditRequest {
    type Res = Vec<types::AuditLogEntry>;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("admin/audit").map_err(AuthError::from)?;
        if let Some(page) = self.page {
            url.query_pairs_mut().append_pair("page", &page.to_string());
        }
        if let Some(per_page) = self.per_page {
            url.query_pairs_mut()
                .append_pair("per_page", &per_page.to_string());
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin Users Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminUsersRequest {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

impl AuthModuleRequest for AdminUsersRequest {
    type Res = types::AdminUsersResponse;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut url = base_url.join("admin/users").map_err(AuthError::from)?;
        if let Some(page) = self.page {
            url.query_pairs_mut().append_pair("page", &page.to_string());
        }
        if let Some(per_page) = self.per_page {
            url.query_pairs_mut()
                .append_pair("per_page", &per_page.to_string());
        }
        Ok(url)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin User Get Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminUserGetRequest {
    pub user_id: String,
}

impl AuthModuleRequest for AdminUserGetRequest {
    type Res = types::UserSchema;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/users/{}", self.user_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin User Update Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct AdminUserUpdateRequest {
    pub user_id: String,
    pub user: crate::admin::UserAttributes,
}

impl AuthModuleRequest for AdminUserUpdateRequest {
    type Res = types::UserSchema;
    type Payload = crate::admin::UserAttributes;

    const METHOD: Method = Method::PUT;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let mut endpoint = base_url.join("admin/users/")?;
        endpoint
            .path_segments_mut()
            .map_err(|()| AuthError::InvalidSession("invalid auth base URL"))?
            .pop_if_empty()
            .push(&self.user_id);
        Ok(endpoint)
    }

    fn payload(&self) -> &Self::Payload {
        &self.user
    }
}

/// Admin User Delete Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminUserDeleteRequest {
    pub user_id: String,
}

impl AuthModuleRequest for AdminUserDeleteRequest {
    type Res = types::UserSchema;
    type Payload = ();

    const METHOD: Method = Method::DELETE;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/users/{}", self.user_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin User Factors Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminUserFactorsRequest {
    pub user_id: String,
}

impl AuthModuleRequest for AdminUserFactorsRequest {
    type Res = Vec<types::MFAFactorSchema>;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/users/{}/factors", self.user_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin User Factor Update Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct AdminUserFactorUpdateRequest {
    pub user_id: String,
    pub factor_id: String,
    pub data: types::MFAFactorUpdateData,
}

impl AuthModuleRequest for AdminUserFactorUpdateRequest {
    type Res = types::MFAFactorSchema;
    type Payload = types::MFAFactorUpdateData;

    const METHOD: Method = Method::PUT;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/users/{}/factors/{}", self.user_id, self.factor_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &self.data
    }
}

/// Admin User Factor Delete Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminUserFactorDeleteRequest {
    pub user_id: String,
    pub factor_id: String,
}

impl AuthModuleRequest for AdminUserFactorDeleteRequest {
    type Res = types::MFAFactorSchema;
    type Payload = ();

    const METHOD: Method = Method::DELETE;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/users/{}/factors/{}", self.user_id, self.factor_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin SSO Providers Get Request
#[derive(Debug, Clone)]
pub struct AdminSsoProvidersGetRequest;

impl AuthModuleRequest for AdminSsoProvidersGetRequest {
    type Res = types::SsoProvidersResponse;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url
            .join("admin/sso/providers")
            .map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin SSO Provider Create Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct AdminSsoProviderCreateRequest {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub metadata_url: Option<String>,
    pub metadata_xml: Option<String>,
    pub domains: Option<Vec<String>>,
    pub attribute_mapping: Option<types::SAMLAttributeMappingSchema>,
}

impl AuthModuleRequest for AdminSsoProviderCreateRequest {
    type Res = types::SSOProviderSchema;
    type Payload = Self;

    const METHOD: Method = Method::POST;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url
            .join("admin/sso/providers")
            .map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Admin SSO Provider Get Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminSsoProviderGetRequest {
    pub sso_provider_id: String,
}

impl AuthModuleRequest for AdminSsoProviderGetRequest {
    type Res = types::SSOProviderSchema;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/sso/providers/{}", self.sso_provider_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Admin SSO Provider Update Request
#[derive(Debug, Clone, Serialize, typed_builder::TypedBuilder)]
pub struct AdminSsoProviderUpdateRequest {
    pub sso_provider_id: String,
    pub metadata_url: Option<String>,
    pub metadata_xml: Option<String>,
    pub domains: Option<Vec<String>>,
    pub attribute_mapping: Option<types::SAMLAttributeMappingSchema>,
}

impl AuthModuleRequest for AdminSsoProviderUpdateRequest {
    type Res = types::SSOProviderSchema;
    type Payload = Self;

    const METHOD: Method = Method::PUT;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/sso/providers/{}", self.sso_provider_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        self
    }
}

/// Admin SSO Provider Delete Request
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct AdminSsoProviderDeleteRequest {
    pub sso_provider_id: String,
}

impl AuthModuleRequest for AdminSsoProviderDeleteRequest {
    type Res = types::SSOProviderSchema;
    type Payload = ();

    const METHOD: Method = Method::DELETE;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        let endpoint = format!("admin/sso/providers/{}", self.sso_provider_id);
        base_url.join(&endpoint).map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}

/// Settings Request
#[derive(Debug, Clone)]
pub struct SettingsRequest;

impl AuthModuleRequest for SettingsRequest {
    type Res = types::SettingsResponse;
    type Payload = ();

    const METHOD: Method = Method::GET;

    fn path(&self, base_url: &Url) -> Result<Url, AuthError> {
        base_url.join("settings").map_err(AuthError::from)
    }

    fn payload(&self) -> &Self::Payload {
        &()
    }
}
