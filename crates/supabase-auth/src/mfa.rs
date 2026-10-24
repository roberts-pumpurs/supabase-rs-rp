//! TOTP and phone MFA. `WebAuthn` is intentionally not implemented here.
use crate::{
    admin::{Delete, Post},
    auth_client::ApiClient,
    error::AuthError,
    types::{
        AccessTokenResponseSchema, FactorDeleteResponse, MFAFactorSchema, MFAFactorStatus,
        TOTPPhoneChallengeResponse,
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use core::fmt;
use serde::{Deserialize, Serialize};

/// MFA handle borrowing the current authenticated client.
#[derive(Clone, Copy)]
pub struct MfaClient<'a> {
    client: &'a ApiClient,
}
impl fmt::Debug for MfaClient<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MfaClient([REDACTED])")
    }
}
#[expect(
    clippy::multiple_inherent_impl,
    reason = "MFA operations are grouped in their dedicated module."
)]
impl ApiClient {
    /// Access MFA using this client's current bearer token.
    #[must_use]
    pub const fn mfa(&self) -> MfaClient<'_> {
        MfaClient { client: self }
    }
}
/// TOTP enrollment secret. Treat both the URI and QR code as credentials.
#[derive(Clone, Deserialize)]
pub struct TotpSecret {
    pub qr_code: String,
    pub secret: String,
    pub uri: String,
}
impl fmt::Debug for TotpSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TotpSecret([REDACTED])")
    }
}
/// Enrollment response for TOTP or phone.
#[derive(Clone, Deserialize)]
pub struct Enrollment {
    pub id: String,
    #[serde(rename = "type")]
    pub factor_type: String,
    pub totp: Option<TotpSecret>,
    pub phone: Option<String>,
}
impl fmt::Debug for Enrollment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Enrollment([REDACTED])")
    }
}
#[derive(Serialize)]
struct Enroll<'a> {
    factor_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    friendly_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issuer: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phone: Option<&'a str>,
}
/// Phone delivery channel.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PhoneChannel {
    Sms,
    Whatsapp,
}
#[derive(Serialize)]
struct Challenge {
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<PhoneChannel>,
}
#[derive(Serialize)]
struct Verify<'a> {
    challenge_id: &'a str,
    code: &'a str,
}
/// Assurance levels parsed from claims, not cryptographically authenticated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssuranceLevel {
    Aal1,
    Aal2,
}
/// Informational assurance state, never an authorization decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assurance {
    pub current: AssuranceLevel,
    pub next: AssuranceLevel,
}
/// Errors inspecting unsigned JWT claims or requesting factors.
#[derive(Debug, thiserror::Error)]
pub enum MfaError {
    #[error("MFA requires an authenticated client")]
    MissingBearer,
    #[error("bearer token contains malformed assurance claims")]
    MalformedClaims,
    #[error(transparent)]
    Auth(#[from] AuthError),
}
#[derive(Deserialize)]
struct Claims {
    aal: AssuranceLevel,
}

impl MfaClient<'_> {
    /// Enroll an unverified TOTP factor. Verify a challenge to activate it.
    /// # Errors
    /// Returns transport, server or response decoding errors.
    pub async fn enroll_totp(
        &self,
        friendly_name: Option<&str>,
        issuer: Option<&str>,
    ) -> Result<Enrollment, AuthError> {
        self.client
            .send(&Post::new(
                &["factors"],
                Enroll {
                    factor_type: "totp",
                    friendly_name,
                    issuer,
                    phone: None,
                },
            ))
            .await
    }
    /// Enroll a phone factor (requires phone MFA enabled on the server).
    /// # Errors
    /// Returns transport, server or response decoding errors.
    pub async fn enroll_phone(
        &self,
        phone: &str,
        friendly_name: Option<&str>,
    ) -> Result<Enrollment, AuthError> {
        self.client
            .send(&Post::new(
                &["factors"],
                Enroll {
                    factor_type: "phone",
                    friendly_name,
                    issuer: None,
                    phone: Some(phone),
                },
            ))
            .await
    }
    /// Challenge a TOTP factor, or select a phone delivery channel.
    /// # Errors
    /// Returns transport, server or response decoding errors.
    pub async fn challenge(
        &self,
        factor_id: &str,
        channel: Option<PhoneChannel>,
    ) -> Result<TOTPPhoneChallengeResponse, AuthError> {
        self.client
            .send(&Post::new(
                &["factors", factor_id, "challenge"],
                Challenge { channel },
            ))
            .await
    }
    /// Verify a challenge and return the upgraded session.
    ///
    /// Construct a new authenticated [`ApiClient`] with the returned `access_token` before
    /// making AAL2 requests. This borrowed client is immutable and keeps its original token.
    /// # Errors
    /// Returns transport, server, invalid-code, invalid-session or response decoding errors.
    pub async fn verify(
        &self,
        factor_id: &str,
        challenge_id: &str,
        code: &str,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        let session = self
            .client
            .send(&Post::new(
                &["factors", factor_id, "verify"],
                Verify { challenge_id, code },
            ))
            .await?;
        crate::session_store::validate_session(&session)?;
        Ok(session)
    }
    /// Remove an enrolled factor.
    /// # Errors
    /// Returns transport, server or response decoding errors.
    pub async fn unenroll(&self, factor_id: &str) -> Result<FactorDeleteResponse, AuthError> {
        self.client
            .send(&Delete::new(&["factors", factor_id], ()))
            .await
    }
    /// Fetch factors from the authenticated user endpoint, including unverified factors.
    /// # Errors
    /// Returns transport, server or response decoding errors.
    pub async fn list_factors(&self) -> Result<Vec<MFAFactorSchema>, AuthError> {
        Ok(self.client.get_user().await?.factors.unwrap_or_default())
    }
    /// Inspect unsigned bearer claims and verified factors to describe the next achievable AAL.
    ///
    /// Does **not** verify signature, issuer, audience or expiration. Never use this result to
    /// authorize requests; the server must validate the bearer token.
    /// # Errors
    /// Returns typed errors for absent/malformed JWTs, or factor lookup failures.
    pub async fn assurance_level(&self) -> Result<Assurance, MfaError> {
        let token = self.client.bearer_token().ok_or(MfaError::MissingBearer)?;
        let mut parts = token.split('.');
        let header = parts.next().ok_or(MfaError::MalformedClaims)?;
        let payload = parts.next().ok_or(MfaError::MalformedClaims)?;
        let signature = parts.next().ok_or(MfaError::MalformedClaims)?;
        if header.is_empty() || signature.is_empty() || parts.next().is_some() {
            return Err(MfaError::MalformedClaims);
        }
        let mut bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_decode_error| MfaError::MalformedClaims)?;
        let claims: Claims =
            simd_json::from_slice(&mut bytes).map_err(|_decode_error| MfaError::MalformedClaims)?;
        let factors = self.list_factors().await?;
        let next = if claims.aal == AssuranceLevel::Aal2
            || factors
                .iter()
                .any(|factor| matches!(factor.status, Some(MFAFactorStatus::Verified)))
        {
            AssuranceLevel::Aal2
        } else {
            AssuranceLevel::Aal1
        };
        Ok(Assurance {
            current: claims.aal,
            next,
        })
    }
}
