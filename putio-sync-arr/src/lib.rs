//! Radarr/Sonarr integration (feature 011): putio-sync answers the Transmission RPC the
//! *arr apps use for a download client, backed by Put.io transfers and the sync's own
//! downloads.

pub mod arr_api;
pub mod contact;
pub mod infohash;
pub mod rpc;
pub mod service;
pub mod view;

pub use arr_api::{ArrApi, ArrApiError, SystemStatus};
pub use contact::Contacts;
pub use service::{
    unix_now, AddOutcome, AddSource, ArrError, ArrService, ItemId, HELD, REFRESH_INTERVAL,
};
