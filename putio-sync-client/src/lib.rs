//! # putio-sync-client
//!
//! Pure API client for Put.io - HTTP communication and API operations.
//!
//! This crate provides a standalone client for interacting with the Put.io API,
//! supporting both OAuth token authentication and app-specific password authentication.
//!
//! ## Features
//!
//! - OAuth token authentication (Bearer)
//! - App-specific password authentication (Basic Auth)
//! - Automatic retry with exponential backoff
//! - Extended timeout support for downloads
//! - Comprehensive API coverage:
//!   - Account operations
//!   - File operations
//!   - Transfer operations
//!   - Delete operations
//!   - Path resolution
//!
//! ## Examples
//!
//! ### Create a client with OAuth token
//!
//! ```no_run
//! use putio_sync_client::PutioApiClient;
//!
//! let client = PutioApiClient::new("your_oauth_token".to_string());
//! ```
//!
//! ### Create a client with app-specific password
//!
//! ```no_run
//! use putio_sync_client::PutioApiClient;
//!
//! let client = PutioApiClient::with_credentials(
//!     "user@example.com".to_string(),
//!     "app_specific_password".to_string()
//! );
//! ```
//!
//! ### List files
//!
//! ```no_run
//! # use putio_sync_client::PutioApiClient;
//! # async fn example() {
//! let client = PutioApiClient::new("your_token".to_string());
//! let files = client.list_files(0).await.unwrap();
//! for file in files.files {
//!     println!("{}: {} bytes", file.name, file.size);
//! }
//! # }
//! ```
//!
//! ### Get account information
//!
//! ```no_run
//! # use putio_sync_client::PutioApiClient;
//! # async fn example() {
//! let client = PutioApiClient::new("your_token".to_string());
//! let account = client.get_account_info().await.unwrap();
//! println!("Username: {}", account.username);
//! println!("Disk used: {}/{} bytes", account.disk.used, account.disk.size);
//! # }
//! ```

pub mod account;
pub mod auth;
pub mod cleanup;
pub mod client;
pub mod delete;
pub mod error;
pub mod files;
pub mod models;
pub mod path;
pub mod probe;
pub mod transfers;
pub mod walk;

// Re-export main types at crate root for convenience
pub use auth::{AuthConfig, AuthMethod};
pub use cleanup::{
    CleanupFailure, EmptyFolderOptions, EmptyFolderReport, FailedStep, RemovedFolder,
};
pub use client::{error_chain, PutioApiClient, RetryPolicy, DEFAULT_BASE_URL, DEFAULT_UPLOAD_URL};
pub use error::{ClientError, ClientResult};
pub use models::{
    AccountInfo, AccountInfoResponse, ApiError, ApiResponse, DiskInfo, FileEntry, FileResponse,
    FilesListResponse, Transfer, TransferResponse, TransfersListResponse,
};
pub use probe::AccountProbe;
pub use transfers::{ReaddError, Readded};
pub use walk::{FileWalk, WalkError, WalkedFile};
