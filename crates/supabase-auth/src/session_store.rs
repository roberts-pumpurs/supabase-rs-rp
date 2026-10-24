//! Session persistence. Stores contain bearer credentials; never share them across projects.
use crate::{error::AuthError, types::AccessTokenResponseSchema};
use core::{future::Future, pin::Pin};
use std::sync::Mutex;

/// Object-safe asynchronous store operation.
pub type StoreFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, SessionStoreError>> + Send + 'a>>;
/// An asynchronous persistence seam. Save must replace the entire previous session.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> StoreFuture<'_, Option<AccessTokenResponseSchema>>;
    fn save<'a>(&'a self, session: &'a AccessTokenResponseSchema) -> StoreFuture<'a, ()>;
    fn clear(&self) -> StoreFuture<'_, ()>;
}
/// Store failures never include serialized credentials.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("session storage I/O failure")]
    Io(#[from] std::io::Error),
    #[error("session storage contains corrupt or incomplete data")]
    Corrupt,
    #[error("session storage lock poisoned")]
    Poisoned,
    #[error("no stored session")]
    Missing,
}
/// Process-local session store; Debug deliberately omits its contents.
#[derive(Default)]
pub struct MemorySessionStore(Mutex<Option<AccessTokenResponseSchema>>);
impl core::fmt::Debug for MemorySessionStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("MemorySessionStore([REDACTED])")
    }
}
impl SessionStore for MemorySessionStore {
    fn load(&self) -> StoreFuture<'_, Option<AccessTokenResponseSchema>> {
        Box::pin(async {
            Ok(self
                .0
                .lock()
                .map_err(|_poisoned| SessionStoreError::Poisoned)?
                .clone())
        })
    }
    fn save<'a>(&'a self, session: &'a AccessTokenResponseSchema) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            *self
                .0
                .lock()
                .map_err(|_poisoned| SessionStoreError::Poisoned)? = Some(session.clone());
            Ok(())
        })
    }
    fn clear(&self) -> StoreFuture<'_, ()> {
        Box::pin(async {
            *self
                .0
                .lock()
                .map_err(|_poisoned| SessionStoreError::Poisoned)? = None;
            Ok(())
        })
    }
}
#[expect(
    clippy::pub_with_shorthand,
    reason = "Rustfmt normalizes crate-private helper visibility to pub(crate)."
)]
pub(crate) fn validate_session(session: &AccessTokenResponseSchema) -> Result<(), AuthError> {
    if session.access_token.as_deref().is_none_or(str::is_empty)
        || session.refresh_token.as_deref().is_none_or(str::is_empty)
        || session.expires_in.is_none_or(|seconds| seconds < 0)
    {
        return Err(AuthError::InvalidSession("missing tokens or expiry"));
    }
    Ok(())
}
/// Filesystem adapter. Atomic replacement, owner-only Unix permissions, no target symlink writes.
///
/// The parent directory must be private and trusted. Concurrent writers are last-writer-wins;
/// coordinate refresh-token rotation externally. This is not an encrypted credential vault.
#[cfg(feature = "file-session-store")]
#[derive(Debug)]
pub struct FileSessionStore {
    path: std::path::PathBuf,
    lock: tokio::sync::Mutex<()>,
}
#[cfg(feature = "file-session-store")]
impl FileSessionStore {
    #[must_use]
    pub fn new<P: Into<std::path::PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            lock: tokio::sync::Mutex::new(()),
        }
    }
}
/// Best-effort cleanup also runs when an asynchronous save is cancelled.
/// Only the directory-entry unlink is synchronous; credential writes remain asynchronous.
#[cfg(feature = "file-session-store")]
struct TemporarySessionFile(std::path::PathBuf);
#[cfg(feature = "file-session-store")]
impl Drop for TemporarySessionFile {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_file(&self.0);
    }
}
#[cfg(feature = "file-session-store")]
impl SessionStore for FileSessionStore {
    fn load(&self) -> StoreFuture<'_, Option<AccessTokenResponseSchema>> {
        Box::pin(async {
            let _guard = self.lock.lock().await;
            let mut bytes = match tokio::fs::read(&self.path).await {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let session = simd_json::from_slice(&mut bytes)
                .map_err(|_decode_error| SessionStoreError::Corrupt)?;
            validate_session(&session).map_err(|_invalid_session| SessionStoreError::Corrupt)?;
            Ok(Some(session))
        })
    }
    fn save<'a>(&'a self, session: &'a AccessTokenResponseSchema) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            use base64::Engine as _;
            use tokio::io::AsyncWriteExt as _;
            let _guard = self.lock.lock().await;
            validate_session(session).map_err(|_invalid_session| SessionStoreError::Corrupt)?;
            let bytes =
                simd_json::to_vec(session).map_err(|_encode_error| SessionStoreError::Corrupt)?;
            let mut random = [0; 16];
            getrandom::fill(&mut random).map_err(|_randomness_error| {
                SessionStoreError::Io(std::io::Error::other("randomness unavailable"))
            })?;
            let name = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random);
            let temporary = self.path.with_file_name(format!(".session-{name}.tmp"));
            // Create synchronously so cancellation cannot outlive an in-flight open and
            // leave a newly created file behind after the cleanup guard has run.
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600)
            };
            let file = options.open(&temporary)?;
            let _temporary_guard = TemporarySessionFile(temporary.clone());
            let mut file = tokio::fs::File::from_std(file);
            let result = async {
                file.write_all(&bytes).await?;
                file.sync_all().await?;
                drop(file);
                // rename replaces the directory entry itself, never follows a destination symlink.
                tokio::fs::rename(&temporary, &self.path).await
            }
            .await;
            // The guard unlinks on failure/cancellation; after rename its path is absent.
            result.map_err(Into::into)
        })
    }
    fn clear(&self) -> StoreFuture<'_, ()> {
        Box::pin(async {
            let _guard = self.lock.lock().await;
            match tokio::fs::remove_file(&self.path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
    }
}
