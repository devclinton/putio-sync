//! System routes
//!
//! Routes for the bandwidth limit, settings (with the configuration assistant), folder rules, statistics, history, the status summary, sign-in, the
//! doctor, event hooks, Lua plugins, What's new, Tailscale, the listen addresses, the Logs page and the DLNA media server

pub mod assistant;
pub mod auth;
pub mod bandwidth;
pub mod doctor;
pub mod history;
pub mod hooks;
pub mod logs;
pub mod media_server;
pub mod network;
pub mod plugins;
pub mod rules;
pub mod settings;
pub mod stats;
pub mod status;
pub mod tailscale;
pub mod verify;
pub mod whats_new;
