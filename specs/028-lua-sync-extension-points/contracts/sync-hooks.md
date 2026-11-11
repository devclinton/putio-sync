# Contract: sync extension points

## Rust (runtime-neutral, `putio_sync_core::sync_hooks`)

```rust
pub enum Hook { Filter, Rename, Folder }
pub struct HookFile { id, name, size, extension, folder, decision, reason }  // Serialize
pub enum FilterAnswer { Download, Stream, Skip }
pub type HookFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait SyncHooks: Send + Sync {
    /// Whether any running plugin provides `hook` (lets the sync skip building HookFile)
    fn provides(&self, hook: Hook) -> bool;
    /// The first valid filter answer and the plugin that gave it
    fn filter<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<(FilterAnswer, String)>>;
    /// The first valid local name
    fn rename<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>>;
    /// The first valid local folder, relative to the download folder
    fn folder<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>>;
}

pub fn parse_filter(&serde_json::Value) -> Result<Option<FilterAnswer>, String>;
pub fn parse_name(&serde_json::Value) -> Result<Option<String>, String>;
pub fn parse_folder(&serde_json::Value) -> Result<Option<String>, String>;
```

An implementation never errors: failures are its own business (log, count, pause) and come
back as `None`.

## Lua

```lua
function plugin.filter(file)  -- "download" | "stream" | "skip" | true | false | nil
function plugin.rename(file)  -- "New name.mkv" | nil
function plugin.folder(file)  -- "TV/Show/Season 01" | nil
```

`file` has the fields of `HookFile` (data-model.md). The functions run with the plugin's
`putio` API, store and approved hosts, under `hook_timeout_ms`.
