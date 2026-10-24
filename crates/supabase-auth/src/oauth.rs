//! Explicit native/server OAuth PKCE handoff; no browser or callback listener is opened.
use crate::{
    auth_client::{
        ApiClient,
        requests::{AuthModuleRequest as _, AuthorizeRequest},
    },
    error::AuthError,
    types::{AccessTokenResponseSchema, CodeChallengeMethod, PkceChallenge},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};

/// Secret verifier. Keep it until the callback code has been exchanged.
pub struct PkceVerifier(String);
impl core::fmt::Debug for PkceVerifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PkceVerifier([REDACTED])")
    }
}
impl PkceVerifier {
    /// Generate 256 bits of operating-system randomness.
    /// # Errors
    /// Returns an error if operating-system randomness is unavailable.
    pub fn generate() -> Result<Self, AuthError> {
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).map_err(|_randomness_error| AuthError::Randomness)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }
    /// Explicit secret handoff for externally managed callback flows.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    #[must_use]
    pub fn challenge(&self) -> PkceChallenge {
        PkceChallenge {
            code_challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(self.0.as_bytes())),
            code_challenge_method: CodeChallengeMethod::S256,
        }
    }
}
/// Authorization URL and its corresponding secret verifier.
#[derive(Debug)]
pub struct OAuthPkce {
    pub authorization_url: url::Url,
    pub verifier: PkceVerifier,
}
#[expect(
    clippy::multiple_inherent_impl,
    reason = "OAuth operations are grouped in their dedicated module."
)]
impl ApiClient {
    /// Start OAuth using the existing authorization request's provider, scopes and redirect.
    /// # Errors
    /// Returns URL or operating-system randomness errors.
    pub fn sign_in_with_oauth(
        &self,
        mut request: AuthorizeRequest,
    ) -> Result<OAuthPkce, AuthError> {
        let verifier = PkceVerifier::generate()?;
        request.pkce = Some(verifier.challenge());
        let authorization_url = request.path(self.auth_url())?;
        Ok(OAuthPkce {
            authorization_url,
            verifier,
        })
    }
    /// Consume the verifier after receiving the callback's code.
    /// # Errors
    /// Returns protocol, transport or invalid-session errors.
    pub async fn complete_oauth(
        &self,
        code: &str,
        verifier: PkceVerifier,
    ) -> Result<AccessTokenResponseSchema, AuthError> {
        let session = self
            .exchange_code_for_session(code, verifier.as_str())
            .await?;
        crate::session_store::validate_session(&session)?;
        Ok(session)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc7636_vector_and_redaction() {
        let verifier = PkceVerifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            verifier.challenge().code_challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(!format!("{verifier:?}").contains(verifier.as_str()));
    }
}
