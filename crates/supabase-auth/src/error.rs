use reqwest::header::InvalidHeaderValue;

use crate::types::ErrorSchema;

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Invalid session: {0}")]
    InvalidSession(&'static str),
    #[error("Session store: {0}")]
    SessionStore(#[from] crate::session_store::SessionStoreError),
    #[error("Operating system randomness unavailable")]
    Randomness,
    #[error("Reqwest error {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Url parse error {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("JSON error {0}")]
    Json(#[from] simd_json::Error),
    #[error("Invalid header value {0}")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
    /// Supabase Auth returned a non-success HTTP status.
    ///
    /// `error` holds the decoded response body. When the body is not a JSON error object,
    /// `error.msg` holds the raw body text.
    #[error("Supabase Auth returned {status}: {error}")]
    Api {
        status: reqwest::StatusCode,
        error: Box<ErrorSchema>,
    },
}
