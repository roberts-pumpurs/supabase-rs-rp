#![cfg_attr(doc, doc = include_str!("../README.md"))]
extern crate alloc;
pub use {futures, redact, url};
pub const SUPABASE_KEY: &str = "apikey";

pub mod admin;
pub mod auth_client;
pub mod error;
pub mod jwt_stream;
pub mod mfa;
pub mod oauth;
pub mod session_store;
pub mod types;
pub use rp_supabase_http::RetryPolicy;
