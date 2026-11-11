//! # putio-sync-plugins
//!
//! Sandboxed Lua plugins run for events on the internal event bus (feature 028).
//!
//! A plugin is a folder with a `plugin.toml` manifest ([`manifest`]) and a Lua script, or
//! one of the plugins bundled in the binary ([`discover`]). Each enabled plugin gets its
//! own thread and Lua state ([`sandbox`]) with a time and memory limit per call, and a
//! host API ([`host`]): logging, its config, HTTP to approved hosts only, a key-value
//! store ([`store`]), time and JSON. [`PluginsService`] feeds them events and keeps the
//! last runs; a failing or slow plugin never holds anything else up.
//!
//! A plugin may also answer the sync's questions while it runs ([`hooks`]): a file
//! filter, a rename rule and a folder rule, through the runtime-free
//! [`putio_sync_core::sync_hooks::SyncHooks`]. A plugin that fails or is slow is passed
//! over and the built-in rule applies.

pub mod action;
pub mod discover;
pub mod hooks;
pub mod host;
pub mod manifest;
pub mod paths;
pub mod sandbox;
pub mod service;
pub mod store;
pub mod worker;

pub use service::{
    ActionAnswer, CallError, PluginState, PluginStatus, PluginsOptions, PluginsService, TestError,
    TEST_MESSAGE,
};
pub use worker::{PluginRun, RunResult};
