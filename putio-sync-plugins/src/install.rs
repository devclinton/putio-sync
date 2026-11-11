//! Installing a plugin into the plugins folder (029): a `plugin.toml` with its entry file,
//! or one `.wasm` that carries its manifest in a `putio-plugin` custom section. The files
//! are written to a hidden folder first, then renamed into place, so a reload never sees
//! half a plugin. An installed plugin starts disabled, like any new folder.

use std::path::{Path, PathBuf};

use crate::discover::{MAX_SCRIPT_BYTES, MAX_WASM_BYTES};
use crate::manifest::Manifest;
use crate::wasm;

/// What to install
pub enum Package {
    /// A manifest and the entry file it names
    Files { manifest: String, entry: Vec<u8> },
    /// A module with the manifest inside it
    Wasm(Vec<u8>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// There is no plugins folder
    NoFolder,
    /// The package is not a plugin; the message says why
    Invalid(String),
    /// A plugin folder with this name exists and `replace` was not set
    Exists(String),
    /// Writing the files failed
    Io(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::NoFolder => write!(f, "no plugins folder is set"),
            InstallError::Invalid(e) | InstallError::Io(e) => write!(f, "{}", e),
            InstallError::Exists(name) => write!(f, "a plugin named \"{}\" is installed", name),
        }
    }
}

/// The manifest and entry of `package`, checked as discovery would
fn unpack(package: Package) -> Result<(Manifest, String, Vec<u8>), InstallError> {
    let invalid = InstallError::Invalid;
    let (text, entry) = match package {
        Package::Files { manifest, entry } => (manifest, entry),
        Package::Wasm(bytes) => match wasm::manifest_section(&bytes).map_err(invalid)? {
            Some(text) => (text, bytes),
            None => {
                return Err(invalid(format!(
                    "the module has no \"{}\" section: send its plugin.toml with it",
                    wasm::SECTION
                )))
            }
        },
    };
    let manifest = Manifest::parse(&text, "plugin.toml").map_err(invalid)?;
    let wasm = manifest.runtime == "wasm";
    let max = if wasm {
        MAX_WASM_BYTES
    } else {
        MAX_SCRIPT_BYTES
    };
    if entry.len() as u64 > max {
        return Err(invalid(format!(
            "{}: larger than {} KiB",
            manifest.entry,
            max / 1024
        )));
    }
    if wasm && !entry.starts_with(wasm::MAGIC) {
        return Err(invalid(format!(
            "{}: not a WebAssembly module",
            manifest.entry
        )));
    }
    if !wasm && std::str::from_utf8(&entry).is_err() {
        return Err(invalid(format!("{}: not UTF-8 text", manifest.entry)));
    }
    Ok((manifest, text, entry))
}

/// Write `package` to `dir/<name>`, replacing a folder of that name only when `replace`
/// is set. Returns the plugin's name.
pub fn install(dir: &Path, package: Package, replace: bool) -> Result<String, InstallError> {
    let (manifest, text, entry) = unpack(package)?;
    let name = manifest.name.clone();
    let target = dir.join(&name);
    if target.exists() && !replace {
        return Err(InstallError::Exists(name));
    }
    let io =
        |what: &Path, e: std::io::Error| InstallError::Io(format!("{}: {}", what.display(), e));
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let temp = hidden(dir, &name, "install");
    let written = std::fs::create_dir(&temp)
        .and_then(|_| std::fs::write(temp.join("plugin.toml"), &text))
        .and_then(|_| std::fs::write(temp.join(&manifest.entry), &entry));
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(io(&temp, e));
    }
    // The old folder moves aside first, so a failed rename leaves it in place
    let old = hidden(dir, &name, "old");
    let moved = target.exists();
    if moved {
        if let Err(e) = std::fs::rename(&target, &old) {
            let _ = std::fs::remove_dir_all(&temp);
            return Err(io(&target, e));
        }
    }
    if let Err(e) = std::fs::rename(&temp, &target) {
        if moved {
            let _ = std::fs::rename(&old, &target);
        }
        let _ = std::fs::remove_dir_all(&temp);
        return Err(io(&target, e));
    }
    if moved {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(name)
}

/// A hidden folder next to the plugins, skipped by discovery
fn hidden(dir: &Path, name: &str, what: &str) -> PathBuf {
    dir.join(format!(".{}-{}-{}", name, what, std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LUA: &str = "name = \"hello\"\nversion = \"1\"\nevents = [\"*\"]\n";

    fn files(manifest: &str, entry: &[u8]) -> Package {
        Package::Files {
            manifest: manifest.to_string(),
            entry: entry.to_vec(),
        }
    }

    #[test]
    fn a_folder_is_written_and_replaced_only_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        let lua = b"return {}";
        assert_eq!(install(&plugins, files(LUA, lua), false).unwrap(), "hello");
        assert_eq!(std::fs::read(plugins.join("hello/main.lua")).unwrap(), lua);
        assert_eq!(
            install(&plugins, files(LUA, b"return {on_event = print}"), false),
            Err(InstallError::Exists("hello".to_string()))
        );
        install(&plugins, files(LUA, b"return {on_event = print}"), true).unwrap();
        let text = std::fs::read_to_string(plugins.join("hello/main.lua")).unwrap();
        assert!(text.contains("print"));
        // No hidden folders are left behind
        let names: Vec<_> = std::fs::read_dir(&plugins)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["hello"]);
    }

    #[test]
    fn a_module_brings_its_own_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = "name = \"w\"\nversion = \"1\"\nruntime = \"wasm\"\nevents = [\"*\"]";
        let module = wat::parse_str(format!(
            "(module (@custom \"putio-plugin\" \"{}\"))",
            manifest.replace('"', "\\\"").replace('\n', "\\n")
        ))
        .unwrap();
        assert_eq!(
            install(dir.path(), Package::Wasm(module.clone()), false).unwrap(),
            "w"
        );
        assert_eq!(
            std::fs::read(dir.path().join("w/plugin.wasm")).unwrap(),
            module
        );
        assert!(std::fs::read_to_string(dir.path().join("w/plugin.toml"))
            .unwrap()
            .contains("runtime = \"wasm\""));
    }

    #[test]
    fn bad_packages_are_refused_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let bare = wat::parse_str("(module)").unwrap();
        let wasm_manifest = "name = \"w\"\nversion = \"1\"\nruntime = \"wasm\"\nevents = [\"*\"]";
        for (package, want) in [
            (Package::Wasm(bare), "no \"putio-plugin\" section"),
            (Package::Wasm(b"hello".to_vec()), "not a WebAssembly module"),
            (files("name = \"../x\"\nversion = \"1\"", b""), "name"),
            (
                files(wasm_manifest, b"not wasm"),
                "not a WebAssembly module",
            ),
            (files(LUA, &[0xff, 0xfe]), "not UTF-8"),
            (
                files(LUA, &vec![b' '; MAX_SCRIPT_BYTES as usize + 1]),
                "larger than",
            ),
        ] {
            match install(dir.path(), package, false) {
                Err(InstallError::Invalid(e)) => assert!(e.contains(want), "{want}: {e}"),
                other => panic!("{want}: {other:?}"),
            }
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
