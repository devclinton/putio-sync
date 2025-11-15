//! Put.io API client
//!
//! This module provides a thin wrapper around the Put.io REST API
//! with authentication, retry logic, and proper error handling.

mod account;
mod client;
mod delete;
mod files;
mod models;
mod path;

pub use client::{ApiClient, DEFAULT_BASE_URL};
pub use models::*;
