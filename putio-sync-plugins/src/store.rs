//! A plugin's key-value store (research R6): one JSON object per plugin in
//! `<config dir>/plugin-data/<name>.json`, written whole through a temp file after each
//! change. In memory when there is no data folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Longest key, in bytes
pub const MAX_KEY_BYTES: usize = 128;
/// Largest value, as JSON
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Largest store, as JSON
pub const MAX_STORE_BYTES: usize = 1024 * 1024;

/// One plugin's store as every worker of the plugin shares it (031): the running one,
/// tests, actions and health checks, so no write is lost to a second copy
pub type SharedStore = std::sync::Arc<std::sync::Mutex<Store>>;

/// One plugin's store
#[derive(Debug)]
pub struct Store {
    path: Option<PathBuf>,
    data: BTreeMap<String, Value>,
}

impl Store {
    /// Open `name`'s store under `dir`; an unreadable file starts empty and is logged
    pub fn open(dir: Option<&Path>, name: &str) -> Store {
        let path = dir.map(|d| d.join(format!("{}.json", name)));
        let data = match path.as_ref().map(std::fs::read_to_string) {
            Some(Ok(text)) => match serde_json::from_str(&text) {
                Ok(data) => data,
                Err(e) => {
                    tracing::warn!(plugin = %name, error = %e, "Plugin store unreadable; starting empty");
                    BTreeMap::new()
                }
            },
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!(plugin = %name, error = %e, "Plugin store unreadable; starting empty");
                BTreeMap::new()
            }
            _ => BTreeMap::new(),
        };
        Store { path, data }
    }

    /// The value under `key`
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// Every key, sorted
    pub fn keys(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    /// Store `value` under `key` and write the file
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return Err(format!("store key must be 1 to {} bytes", MAX_KEY_BYTES));
        }
        let size = serde_json::to_vec(&value).map_err(|e| e.to_string())?.len();
        if size > MAX_VALUE_BYTES {
            return Err(format!(
                "store value larger than {} KiB",
                MAX_VALUE_BYTES / 1024
            ));
        }
        let old = self.data.insert(key.to_string(), value);
        let total = serde_json::to_vec(&self.data)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if total > MAX_STORE_BYTES {
            match old {
                Some(v) => self.data.insert(key.to_string(), v),
                None => self.data.remove(key),
            };
            return Err(format!("store full ({} KiB)", MAX_STORE_BYTES / 1024));
        }
        self.write()
    }

    /// Remove `key` and write the file
    pub fn delete(&mut self, key: &str) -> Result<(), String> {
        if self.data.remove(key).is_some() {
            self.write()?;
        }
        Ok(())
    }

    fn write(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let fail = |e: std::io::Error| format!("store not saved: {}", e.kind());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        let text = serde_json::to_vec_pretty(&self.data).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(fail)?;
        std::fs::rename(&tmp, path).map_err(fail)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn kept_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(Some(dir.path()), "ntfy");
        s.set("seen", json!(["a", "b"])).unwrap();
        s.set("n", json!(3)).unwrap();
        s.delete("n").unwrap();
        let again = Store::open(Some(dir.path()), "ntfy");
        assert_eq!(again.get("seen"), Some(&json!(["a", "b"])));
        assert_eq!(again.keys(), vec!["seen"]);
        assert!(!dir.path().join("ntfy.json.tmp").exists());
        assert!(Store::open(Some(dir.path()), "other").keys().is_empty());
    }

    #[test]
    fn limits() {
        let mut s = Store::open(None, "x");
        assert!(s.set("", json!(1)).unwrap_err().contains("1 to 128 bytes"));
        assert!(s.set(&"k".repeat(129), json!(1)).is_err());
        s.set(&"k".repeat(128), json!(1)).unwrap();
        let big = "x".repeat(MAX_VALUE_BYTES);
        assert!(s
            .set("big", json!(big))
            .unwrap_err()
            .contains("larger than 64 KiB"));
        let chunk = "x".repeat(60 * 1024);
        let mut full = None;
        for i in 0..20 {
            if let Err(e) = s.set(&format!("k{}", i), json!(chunk)) {
                full = Some((i, e));
                break;
            }
        }
        let (i, e) = full.expect("store fills up");
        assert!(e.contains("store full (1024 KiB)"));
        assert!(s.get(&format!("k{}", i)).is_none());
    }

    #[test]
    fn a_broken_file_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.json"), "{not json").unwrap();
        assert!(Store::open(Some(dir.path()), "x").keys().is_empty());
    }
}
