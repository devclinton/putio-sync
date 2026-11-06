//! Radarr/Sonarr integration (feature 011): putio-sync answers the Transmission RPC the
//! *arr apps use for a download client, backed by Put.io transfers and the sync's own
//! downloads.

pub mod arr_api;
pub mod clients;
pub mod contact;
pub mod discovery;
pub mod infohash;
pub mod media;
pub mod path_check;
pub mod path_mapping;
pub mod recommend;
pub mod rpc;
pub mod service;
pub mod setup;
pub mod view;

pub use arr_api::{ArrApi, ArrApiError, DownloadClient, SystemStatus};
pub use contact::Contacts;
pub use media::ArrMedia;
pub use service::{
    unix_now, AddOutcome, AddSource, ArrError, ArrService, ItemId, HELD, REFRESH_INTERVAL,
};
