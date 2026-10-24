#![cfg_attr(doc, doc = include_str!("../README.md"))]

extern crate alloc;

mod connection;
mod driver;
pub use driver::{ConnectionOptions, ConnectionStatus, RealtimeStream};
pub mod error;
pub mod message;
pub mod realtime;

pub use {futures, rp_supabase_auth, simd_json, url};
