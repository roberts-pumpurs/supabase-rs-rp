#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![expect(
    clippy::multiple_inherent_impl,
    reason = "Object operations, image transformations, and TUS sessions have separate modules"
)]
mod bucket;
mod client;
mod error;
mod image;
mod path;
mod resumable;
mod types;

pub use image::{ImageFormat, ResizeMode, TransformOptions};
pub use resumable::{ResumableUpload, TUS_CHUNK_SIZE};
pub use rp_supabase_http::RetryPolicy;

pub use bucket::{Bucket, ObjectDownload};
pub use client::StorageClient;
pub use error::{ApiErrorBody, PathError, StorageError, StorageErrorBody, StorageErrorCode};
pub use types::{
    BucketInfo, BucketOptions, FileObject, FileOptions, ListOptions, ObjectKey, SignedUrl, SortBy,
    SortColumn, SortOrder,
};
pub use {bytes, reqwest, url};
