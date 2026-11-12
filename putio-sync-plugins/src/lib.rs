//! # putio-sync-plugins
//!
//! Sandboxed Lua (feature 028) and WebAssembly (029) plugins run for events on the
//! internal event bus.
//!
//! A plugin is a folder with a `plugin.toml` manifest ([`manifest`]) and a Lua script or a
//! `.wasm` module, or one of the plugins bundled in the binary ([`discover`]); [`install`]
//! adds one from a file or a URL. Each enabled plugin gets its own thread and Lua state
//! ([`sandbox`]) or WebAssembly instance ([`wasm`]) with a time and memory limit per call,
//! and one host API ([`host`]): logging, its config, HTTP to approved hosts only, approved
//! folders only, a key-value store ([`store`]), time and JSON. [`PluginsService`] feeds them events and keeps the
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
pub mod install;
pub mod manifest;
pub mod paths;
pub mod sandbox;
pub mod service;
pub mod source;
pub mod store;
pub mod wasm;
pub mod workbench;
pub mod worker;

pub use service::{
    ActionAnswer, CallError, Links, PluginState, PluginStatus, PluginsOptions, PluginsService,
    TestError, TEST_MESSAGE,
};
pub use worker::{PluginRun, RunResult};
