use reqwest::StatusCode;
use serde::{Deserialize, Deserializer};

/// Errors returned by [`StorageClient`](crate::StorageClient) and [`Bucket`](crate::Bucket).
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The Storage API answered with a non-success HTTP status.
    #[error("storage API error ({status}): {body}")]
    Api {
        /// HTTP status of the response.
        status: StatusCode,
        /// Decoded response body.
        body: ApiErrorBody,
    },
    /// A bucket id or object path is not valid.
    #[error("invalid storage path {path:?}: {reason}")]
    InvalidPath {
        /// The rejected input.
        path: String,
        /// Why the input was rejected.
        reason: PathError,
    },
    /// The project URL cannot carry path segments (for example `mailto:`).
    #[error("project URL cannot be a base URL")]
    InvalidBaseUrl,
    /// A resumable upload or image option violated the protocol.
    #[error("storage protocol error: {0}")]
    Protocol(&'static str),
    /// The Storage API returned a signed URL without a `?token=` query.
    #[error("signed URL from the storage API has no token: {0:?}")]
    MissingSignedToken(String),
    /// The Storage API did not sign an object, for example because it does not exist or the
    /// caller cannot read it.
    #[error("storage API did not sign {path:?}: {message}")]
    SignFailed {
        /// The requested object path.
        path: String,
        /// The reason from the Storage API.
        message: String,
    },
    /// A URL could not be parsed or joined.
    #[error(transparent)]
    Url(#[from] url::ParseError),
    /// The HTTP transport failed.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// A success response body is not the expected JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A key, token, or option is not a valid HTTP header value.
    #[error(transparent)]
    InvalidHeader(#[from] reqwest::header::InvalidHeaderValue),
}

impl StorageError {
    /// Status the Storage API reports for an [`StorageError::Api`] error, `None` for other
    /// errors.
    ///
    /// The object routes answer most errors with HTTP `400` and put the real status in the
    /// body's `statusCode`, for example `"404"` for a missing object and `"403"` when row level
    /// security denies access. This returns the body status when it is a valid status code and
    /// the HTTP status otherwise.
    #[must_use]
    pub fn api_status(&self) -> Option<StatusCode> {
        let Self::Api { status, body } = self else {
            return None;
        };
        let reported = match body {
            ApiErrorBody::Storage(StorageErrorBody {
                status_code: Some(code),
                ..
            }) => code.parse::<u16>().ok(),
            ApiErrorBody::Storage(_) | ApiErrorBody::Raw(_) => None,
        };
        Some(
            reported
                .and_then(|code| StatusCode::from_u16(code).ok())
                .unwrap_or(*status),
        )
    }

    /// Machine-readable `code` of an [`StorageError::Api`] error body, `None` for other
    /// errors and for bodies without a code.
    ///
    /// For example, an upload without upsert to an existing path fails with
    /// [`StorageErrorCode::KeyAlreadyExists`] or [`StorageErrorCode::ResourceAlreadyExists`],
    /// and a delete that row level security blocks fails with
    /// [`StorageErrorCode::AccessDenied`].
    #[must_use]
    pub const fn code(&self) -> Option<&StorageErrorCode> {
        let Self::Api {
            body: ApiErrorBody::Storage(body),
            ..
        } = self
        else {
            return None;
        };
        body.code.as_ref()
    }
}

/// Reason for [`StorageError::InvalidPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// The path is empty.
    #[error("path is empty")]
    Empty,
    /// The path contains an empty segment (`a//b`, a leading or a trailing `/`).
    #[error("path contains an empty segment")]
    EmptySegment,
    /// The path contains a `.` or `..` segment.
    #[error("path contains a `.` or `..` segment")]
    DotSegment,
    /// The path contains a tab, carriage return, or line feed. URL parsing drops these
    /// characters silently, which changes the target object.
    #[error("path contains a tab, carriage return, or line feed")]
    ControlCharacter,
}

/// Body of a failed Storage API response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiErrorBody {
    /// The standard Storage error JSON.
    Storage(StorageErrorBody),
    /// Any other body, kept as text.
    Raw(String),
}

impl ApiErrorBody {
    /// Decodes the standard error JSON. Text, and JSON with none of its fields, stay `Raw`.
    pub(crate) fn decode(text: String) -> Self {
        match serde_json::from_str::<StorageErrorBody>(&text) {
            Ok(body)
                if body.status_code.is_some()
                    || body.code.is_some()
                    || body.error.is_some()
                    || body.message.is_some() =>
            {
                Self::Storage(body)
            }
            Ok(_) | Err(_) => Self::Raw(text),
        }
    }
}

impl core::fmt::Display for ApiErrorBody {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Storage(body) => {
                let label = body
                    .code
                    .as_ref()
                    .map(StorageErrorCode::as_str)
                    .or(body.error.as_deref());
                match (label, body.message.as_deref()) {
                    (Some(label), Some(message)) => write!(formatter, "{label}: {message}"),
                    (Some(text), None) | (None, Some(text)) => formatter.write_str(text),
                    (None, None) => Ok(()),
                }
            }
            Self::Raw(text) => formatter.write_str(text),
        }
    }
}

/// Standard Storage error JSON, for example
/// `{"statusCode": "409", "code": "KeyAlreadyExists", "error": "Duplicate", "message": "..."}`.
/// Every field is optional because some responses omit them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StorageErrorBody {
    /// Status code reported in the body. The API sends it as a string; numbers are converted.
    #[serde(rename = "statusCode", default, deserialize_with = "string_or_number")]
    pub status_code: Option<String>,
    /// Machine-readable error code, for example `KeyAlreadyExists`.
    #[serde(default)]
    pub code: Option<StorageErrorCode>,
    /// Short error name, for example `not_found` or `Duplicate`.
    #[serde(default)]
    pub error: Option<String>,
    /// Human-readable message.
    #[serde(default)]
    pub message: Option<String>,
}

fn string_or_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Code {
        Text(String),
        Number(u64),
    }
    Ok(
        Option::<Code>::deserialize(deserializer)?.map(|code| match code {
            Code::Text(text) => text,
            Code::Number(number) => number.to_string(),
        }),
    )
}

/// Declares [`StorageErrorCode`] with one variant per wire value, plus `Other`.
macro_rules! storage_error_codes {
    ($($variant:ident => $wire:literal,)+) => {
        /// Machine-readable `code` of a Storage API error, from the `ErrorCode` enum of
        /// supabase/storage. A code this crate does not know is kept in `Other`.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
        #[serde(from = "String")]
        #[non_exhaustive]
        pub enum StorageErrorCode {
            $(
                #[doc = concat!("`", $wire, "`")]
                $variant,
            )+
            /// Any other code.
            Other(String),
        }

        impl StorageErrorCode {
            /// Wire value of the code.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)+
                    Self::Other(code) => code,
                }
            }
        }

        impl From<String> for StorageErrorCode {
            fn from(code: String) -> Self {
                match code.as_str() {
                    $($wire => Self::$variant,)+
                    _ => Self::Other(code),
                }
            }
        }
    };
}

storage_error_codes! {
    NoSuchBucket => "NoSuchBucket",
    NoSuchKey => "NoSuchKey",
    NoSuchUpload => "NoSuchUpload",
    InvalidJwt => "InvalidJWT",
    InvalidRequest => "InvalidRequest",
    InvalidArgument => "InvalidArgument",
    TenantNotFound => "TenantNotFound",
    EntityTooLarge => "EntityTooLarge",
    EntityTooSmall => "EntityTooSmall",
    InternalError => "InternalError",
    ResourceAlreadyExists => "ResourceAlreadyExists",
    ResourceNotEmpty => "ResourceNotEmpty",
    InvalidBucketName => "InvalidBucketName",
    InvalidKey => "InvalidKey",
    InvalidRange => "InvalidRange",
    InvalidMimeType => "InvalidMimeType",
    InvalidUploadId => "InvalidUploadId",
    KeyAlreadyExists => "KeyAlreadyExists",
    BucketAlreadyExists => "BucketAlreadyExists",
    DatabaseTimeout => "DatabaseTimeout",
    DatabaseReadOnly => "DatabaseReadOnly",
    InvalidSignature => "InvalidSignature",
    ExpiredToken => "ExpiredToken",
    SignatureDoesNotMatch => "SignatureDoesNotMatch",
    AccessDenied => "AccessDenied",
    ResourceLocked => "ResourceLocked",
    ResourceReferenced => "ResourceReferenced",
    DatabaseError => "DatabaseError",
    MissingContentLength => "MissingContentLength",
    MissingParameter => "MissingParameter",
    InvalidParameter => "InvalidParameter",
    InvalidUploadSignature => "InvalidUploadSignature",
    LockTimeout => "LockTimeout",
    PreconditionFailed => "PreconditionFailed",
    InvalidChecksum => "InvalidChecksum",
    SlowDown => "SlowDown",
    FeatureNotEnabled => "FeatureNotEnabled",
    NotSupported => "NotSupported",
    UnknownError => "UnknownError",
}

impl core::fmt::Display for StorageErrorCode {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
