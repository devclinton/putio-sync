//! Embeds the CLI's translation catalogs (feature 060). With the `i18n` feature, the keys
//! of `i18n/en.json` are packed once (`KEYS`, a deflated JSON array, sorted) and each other
//! `i18n/<code>.json` as a deflated JSON array of its values in that order, `null` where it
//! has none (`CATALOGS`). The keys are the English text, so storing them once keeps the
//! binary small. Without the feature both are empty and the binary has English only.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=i18n");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut list = String::new();
    let mut keys_line = String::from("pub static KEYS: &[u8] = &[];\n");
    let mut entries = String::new();
    if std::env::var_os("CARGO_FEATURE_I18N").is_some() {
        let dir = out.join("i18n");
        std::fs::create_dir_all(&dir).expect("create the catalog folder");
        let pack = |name: &str, json: &[u8]| -> String {
            let packed = dir.join(name);
            let mut encoder = flate2::write::DeflateEncoder::new(
                std::fs::File::create(&packed).expect("create a packed catalog"),
                flate2::Compression::best(),
            );
            encoder.write_all(json).expect("pack a catalog");
            encoder.finish().expect("finish a packed catalog");
            format!("include_bytes!({:?})", packed.display().to_string())
        };
        let read = |file: &PathBuf| -> BTreeMap<String, String> {
            let text = std::fs::read(file).expect("read a catalog");
            serde_json::from_slice(&text).unwrap_or_else(|e| {
                panic!("{} is not a JSON object of strings: {e}", file.display())
            })
        };
        let english = read(&PathBuf::from("i18n/en.json"));
        let keys: Vec<&String> = english.keys().collect();
        let json = serde_json::to_vec(&keys).expect("encode the keys");
        keys_line = format!("pub static KEYS: &[u8] = {};\n", pack("keys.json.z", &json));
        let mut files: Vec<PathBuf> = std::fs::read_dir("i18n")
            .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        files.sort();
        for file in files {
            let code = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if file.extension().and_then(|e| e.to_str()) != Some("json") || code == "en" {
                continue;
            }
            let catalog = read(&file);
            let values: Vec<Option<&String>> = keys.iter().map(|key| catalog.get(*key)).collect();
            let json = serde_json::to_vec(&values).expect("encode a catalog");
            entries.push_str(&format!(
                "    ({code:?}, {}),\n",
                pack(&format!("{code}.json.z"), &json)
            ));
        }
    }
    list.push_str(&keys_line);
    list.push_str("pub static CATALOGS: &[(&str, &[u8])] = &[\n");
    list.push_str(&entries);
    list.push_str("];\n");
    std::fs::write(out.join("i18n_catalogs.rs"), list).expect("write the catalog list");
}
