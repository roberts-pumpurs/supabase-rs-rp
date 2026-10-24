use base64::Engine as _;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Bucket, ObjectDownload, StorageError};

/// How an image fits the requested dimensions.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResizeMode {
    /// Crop to cover both dimensions.
    Cover,
    /// Fit inside both dimensions.
    Contain,
    /// Stretch to both dimensions.
    Fill,
}

/// Output format supported by Supabase's transformation interface.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// Keep the original image format instead of automatic WebP negotiation.
    Origin,
}

/// Image transformation options. Omitted values use server defaults.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct TransformOptions {
    /// Target width in pixels (1–2500).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Target height in pixels (1–2500).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Resize algorithm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resize: Option<ResizeMode>,
    /// Quality (20–100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    /// Output format preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ImageFormat>,
}

impl TransformOptions {
    fn validate(&self) -> Result<(), StorageError> {
        if [self.width, self.height]
            .into_iter()
            .flatten()
            .any(|dimension| !(1..=2500).contains(&dimension))
            || self
                .quality
                .is_some_and(|quality| !(20..=100).contains(&quality))
        {
            return Err(StorageError::Protocol(
                "image dimensions or quality out of range",
            ));
        }
        Ok(())
    }

    fn apply(&self, url: &mut Url) -> Result<(), StorageError> {
        self.validate()?;
        let value = serde_json::to_value(self)?;
        if let Some(options) = value.as_object() {
            for (key, value) in options {
                let value = value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned);
                url.query_pairs_mut().append_pair(key, &value);
            }
        }
        Ok(())
    }
}

/// Checks the remote claim without claiming to verify its cryptographic signature.
fn validate_signed_path(token: &str, bucket: &str, path: &str) -> Result<(), StorageError> {
    #[derive(Deserialize)]
    struct Claims {
        url: String,
    }
    let invalid = || StorageError::Protocol("signed image response has invalid token claims");
    let mut parts = token.split('.');
    let header = parts.next().ok_or_else(invalid)?;
    let payload = parts.next().ok_or_else(invalid)?;
    let signature = parts.next().ok_or_else(invalid)?;
    if header.is_empty() || signature.is_empty() || parts.next().is_some() {
        return Err(invalid());
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_error| invalid())?;
    let claims: Claims = serde_json::from_slice(&payload).map_err(|_error| invalid())?;
    // The server compares this claim to the decoded bucket/key, not an encoded URL.
    // Do not decode either side: literal percent escapes are valid object-name bytes.
    if claims
        .url
        .strip_prefix(bucket)
        .and_then(|tail| tail.strip_prefix('/'))
        != Some(path)
    {
        return Err(StorageError::Protocol(
            "remote signed-path mismatch: signed image token does not match requested bucket/key",
        ));
    }
    Ok(())
}

impl Bucket<'_> {
    /// Builds a transformed public image URL without a request.
    ///
    /// # Errors
    /// Returns an error for invalid paths or transformation values.
    pub fn public_transformed_url(
        &self,
        path: &str,
        options: &TransformOptions,
    ) -> Result<Url, StorageError> {
        let mut url = self.object_url(&["render", "image", "public"], path)?;
        options.apply(&mut url)?;
        Ok(url)
    }

    /// Streams a public transformed image.
    ///
    /// # Errors
    /// Returns path, option, transport, or server errors.
    pub async fn download_public_transformed(
        &self,
        path: &str,
        options: &TransformOptions,
    ) -> Result<ObjectDownload, StorageError> {
        let url = self.public_transformed_url(path, options)?;
        Ok(ObjectDownload::new(
            self.client()
                .send(self.client().request(Method::GET, url))
                .await?,
        ))
    }

    /// Streams a transformed image under the current user's authorization.
    ///
    /// # Errors
    /// Returns path, option, transport, or server errors.
    pub async fn download_transformed(
        &self,
        path: &str,
        options: &TransformOptions,
    ) -> Result<ObjectDownload, StorageError> {
        let mut url = self.object_url(&["render", "image", "authenticated"], path)?;
        options.apply(&mut url)?;
        Ok(ObjectDownload::new(
            self.client()
                .send(self.client().request(Method::GET, url))
                .await?,
        ))
    }

    /// Signs an image transformation. The signature binds the transformation options.
    ///
    /// # Errors
    /// Returns path, option, transport, server, missing-token, or remote signed-path mismatch errors.
    pub async fn create_signed_transformed_url(
        &self,
        path: &str,
        expires_in: u64,
        options: &TransformOptions,
    ) -> Result<Url, StorageError> {
        #[derive(Deserialize)]
        struct Signed {
            #[serde(rename = "signedURL")]
            url: String,
        }
        options.validate()?;
        let endpoint = self.object_url(&["object", "sign"], path)?;
        let signed: Signed = self
            .client()
            .json(
                self.client()
                    .request(Method::POST, endpoint)
                    .json(&serde_json::json!({"expiresIn": expires_in, "transform": options})),
            )
            .await?;
        let (_, token) = signed
            .url
            .rsplit_once("?token=")
            .ok_or(StorageError::Protocol("signed image response has no token"))?;
        let token = url::form_urlencoded::parse(format!("token={token}").as_bytes())
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.into_owned())
            .ok_or(StorageError::Protocol("signed image response has no token"))?;
        validate_signed_path(&token, self.id(), path)?;
        let mut url = self.object_url(&["render", "image", "sign"], path)?;
        url.query_pairs_mut().append_pair("token", &token);
        Ok(url)
    }
}
