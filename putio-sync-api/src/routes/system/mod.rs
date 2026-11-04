//! System routes
//!
//! Routes for the bandwidth limit, settings (with the configuration assistant), folder rules, statistics, history, the status summary, sign-in, the
//! doctor, event hooks, Lua plugins, What's new, Tailscale and the listen addresses

pub mod assistant;
pub mod auth;
pub mod bandwidth;
pub mod doctor;
pub mod history;
pub mod hooks;
pub mod network;
pub mod plugins;
pub mod rules;
pub mod settings;
pub mod stats;
pub mod status;
pub mod tailscale;
pub mod whats_new;
