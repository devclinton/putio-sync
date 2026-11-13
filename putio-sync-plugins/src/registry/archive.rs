//! A registry release's file, checked before anything is written (059): the SHA-256 the
//! index pins, then a `.tar.gz` read in memory (one top folder, plain files, no `..`, size
//! and count limits) or a `.wasm` that carries its manifest, then the manifest compared
//! with the entry, so what installs is what the index described.

use std::io::Read;
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

use super::index::{Entry, Release};
use crate::install::Package;
use crate::manifest::Manifest;
use crate::wasm;

/// The largest release file, and the most an archive may unpack to
pub const MAX_ARCHIVE_BYTES: usize = 5 * 1024 * 1024;

/// The most files an archive may hold
pub const MAX_FILES: usize = 200;

/// A release that passed every check
pub struct Checked {
    pub manifest: Manifest,
    pub package: Package,
    pub sha256: String,
}

/// Lowercase hex SHA-256 of `bytes`
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Check `bytes`, downloaded from `release.url`, against `release` and `entry`
pub fn check(bytes: &[u8], entry: &Entry, release: &Release) -> Result<Checked, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("the release is larger than 5 MB".to_string());
    }
    let digest = sha256(bytes);
    if digest != release.sha256 {
        return Err(format!(
            "the release's SHA-256 is {}, the registry says {}",
            digest, release.sha256
        ));
    }
    let (text, package) = if release.url.ends_with(".wasm") {
        let text = wasm::manifest_section(bytes)?
            .ok_or_else(|| format!("the module has no \"{}\" section", wasm::SECTION))?;
        (text, Package::Wasm(bytes.to_vec()))
    } else {
        let files = untar(bytes)?;
        let file = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, b)| b);
        let text = file("plugin.toml").ok_or("the archive has no plugin.toml in its top folder")?;
        let text = String::from_utf8(text.clone()).map_err(|_| "plugin.toml: not UTF-8")?;
        let parsed = Manifest::parse(&text, "plugin.toml")?;
        let script = file(&parsed.entry)
            .ok_or_else(|| format!("the archive has no {}", parsed.entry))?
            .clone();
        let package = Package::Files {
            manifest: text.clone(),
            entry: script,
        };
        (text, package)
    };
    let manifest = Manifest::parse(&text, "plugin.toml")?;
    compare(&manifest, entry, release)?;
    Ok(Checked {
        manifest,
        package,
        sha256: digest,
    })
}

/// Every file of a `.tar.gz` as (path inside the top folder, bytes)
fn untar(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = |e: std::io::Error| format!("the archive cannot be read: {}", e);
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut files = Vec::new();
    let mut top: Option<String> = None;
    let mut total = 0usize;
    for item in archive.entries().map_err(bad)? {
        let mut item = item.map_err(bad)?;
        let path = item.path().map_err(bad)?.into_owned();
        let shown = path.display().to_string();
        let kind = item.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err(format!("{}: only plain files are allowed", shown));
        }
        let parts: Vec<String> = match plain_parts(&path) {
            Some(parts) if parts.len() >= 2 => parts,
            _ => {
                return Err(format!(
                    "{}: every file must be inside one top folder",
                    shown
                ))
            }
        };
        match &top {
            Some(t) if *t != parts[0] => {
                return Err("the archive must hold exactly one top folder".to_string())
            }
            None => top = Some(parts[0].clone()),
            _ => {}
        }
        if files.len() == MAX_FILES {
            return Err(format!("the archive holds more than {} files", MAX_FILES));
        }
        let mut body = Vec::new();
        let limit = (MAX_ARCHIVE_BYTES - total) as u64 + 1;
        item.by_ref()
            .take(limit)
            .read_to_end(&mut body)
            .map_err(bad)?;
        total += body.len();
        if total > MAX_ARCHIVE_BYTES {
            return Err("the archive unpacks to more than 5 MB".to_string());
        }
        files.push((parts[1..].join("/"), body));
    }
    Ok(files)
}

/// The parts of a relative path with no `..`, root or prefix
fn plain_parts(path: &Path) -> Option<Vec<String>> {
    path.components()
        .map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().to_string()),
            _ => None,
        })
        .filter(|p| p.as_deref() != Some("."))
        .collect()
}

/// The manifest says what the entry says it does
fn compare(m: &Manifest, entry: &Entry, release: &Release) -> Result<(), String> {
    let sorted = |v: &[String]| {
        let mut v = v.to_vec();
        v.sort();
        v
    };
    let differ = |what: &str, got: String, want: String| {
        Err(format!(
            "plugin.toml {} is {}, the registry says {}",
            what, got, want
        ))
    };
    if m.name != entry.name {
        return differ("name", m.name.clone(), entry.name.clone());
    }
    if m.version != release.version {
        return differ("version", m.version.clone(), release.version.clone());
    }
    if m.runtime != entry.runtime {
        return differ("runtime", m.runtime.clone(), entry.runtime.clone());
    }
    for (what, got, want) in [
        ("events", &m.events, &entry.events),
        ("hooks", &m.hooks, &entry.hooks),
        ("hosts", &m.permissions.http.hosts, &entry.hosts.fixed),
        (
            "host_from_config",
            &m.permissions.http.host_from_config,
            &entry.hosts.from_config,
        ),
    ] {
        if sorted(got) != sorted(want) {
            return differ(what, format!("{:?}", got), format!("{:?}", want));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const MANIFEST: &str = "name = \"hook\"\nversion = \"1.0.0\"\nevents = [\"download.finished\"]\n\n[permissions.http]\nhost_from_config = [\"url\"]\n\n[[config]]\nkey = \"url\"\ntype = \"url\"\n";

    /// A `.tar.gz` of `files`, each (path, bytes)
    pub fn targz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, body) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            // set_path refuses `..`; write the name field as an attacker would
            let name = path.as_bytes();
            header.as_old_mut().name[..name.len()].copy_from_slice(name);
            header.set_cksum();
            builder.append(&header, *body).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    pub fn good() -> Vec<u8> {
        targz(&[
            ("hook/plugin.toml", MANIFEST.as_bytes()),
            ("hook/main.lua", b"return {on_event = function() end}"),
            ("hook/README.md", b"# hook"),
        ])
    }

    pub fn entry_for(bytes: &[u8]) -> (Entry, Release) {
        let release = Release {
            version: "1.0.0".into(),
            url: "https://x.org/hook-1.0.0.tar.gz".into(),
            sha256: sha256(bytes),
            min_putio_sync: "0.1.0".into(),
            notes: None,
        };
        let entry = Entry {
            name: "hook".into(),
            description: "d".into(),
            author: "ada".into(),
            repository: "https://x.org".into(),
            license: "MIT".into(),
            runtime: "lua".into(),
            events: vec!["download.finished".into()],
            hosts: super::super::index::Hosts {
                fixed: vec![],
                from_config: vec!["url".into()],
            },
            releases: vec![release.clone()],
            ..Default::default()
        };
        (entry, release)
    }

    fn refused(bytes: Vec<u8>) -> String {
        let (entry, release) = entry_for(&bytes);
        check(&bytes, &entry, &release).err().expect("refused")
    }

    #[test]
    fn a_good_archive_passes() {
        let bytes = good();
        let (entry, release) = entry_for(&bytes);
        let checked = check(&bytes, &entry, &release).unwrap();
        assert_eq!(checked.manifest.name, "hook");
        let Package::Files { entry, .. } = checked.package else {
            panic!("not files")
        };
        assert!(entry.starts_with(b"return"));
    }

    #[test]
    fn a_wrong_hash_is_refused_first() {
        let bytes = good();
        let (entry, mut release) = entry_for(&bytes);
        release.sha256 = "0".repeat(64);
        assert!(check(&bytes, &entry, &release)
            .err()
            .expect("refused")
            .contains("SHA-256"));
    }

    #[test]
    fn unsafe_archives_are_refused() {
        let lua: &[u8] = b"return {}";
        let toml = MANIFEST.as_bytes();
        assert!(
            refused(targz(&[("hook/../x", lua), ("hook/plugin.toml", toml)]))
                .contains("inside one top folder")
        );
        assert!(refused(targz(&[("/etc/x", lua)])).contains("inside one top folder"));
        assert!(refused(targz(&[("plugin.toml", toml)])).contains("inside one top folder"));
        assert!(
            refused(targz(&[("a/plugin.toml", toml), ("b/main.lua", lua)]))
                .contains("one top folder")
        );
        let many: Vec<(String, &[u8])> = (0..=MAX_FILES)
            .map(|i| (format!("hook/{}", i), lua))
            .collect();
        let many: Vec<(&str, &[u8])> = many.iter().map(|(n, b)| (n.as_str(), *b)).collect();
        assert!(refused(targz(&many)).contains("more than 200 files"));
        let big = vec![b'x'; MAX_ARCHIVE_BYTES];
        assert!(refused(targz(&[("hook/a", &big), ("hook/b", lua)])).contains("5 MB"));
        assert!(refused(targz(&[("hook/plugin.toml", toml)])).contains("no main.lua"));

        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_path("hook/link").unwrap();
        header.set_link_name("/etc/passwd").unwrap();
        header.set_size(0);
        header.set_cksum();
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        builder.append(&header, std::io::empty()).unwrap();
        let link = builder.into_inner().unwrap().finish().unwrap();
        assert!(refused(link).contains("only plain files"));
    }

    #[test]
    fn the_manifest_must_match_the_entry() {
        let bytes = good();
        let (mut entry, release) = entry_for(&bytes);
        entry.events = vec!["download.failed".into()];
        assert!(check(&bytes, &entry, &release)
            .err()
            .expect("refused")
            .contains("events"));
        let (mut entry, release) = entry_for(&bytes);
        entry.hosts.fixed = vec!["api.example.com".into()];
        assert!(check(&bytes, &entry, &release)
            .err()
            .expect("refused")
            .contains("hosts"));
        let (entry, mut release) = entry_for(&bytes);
        release.version = "2.0.0".into();
        assert!(check(&bytes, &entry, &release)
            .err()
            .expect("refused")
            .contains("version"));
    }
}
