//! The shipped example configs and every ```toml block in the README load and lint clean
//! (no unknown-key warnings, no parse errors).

use putio_sync_core::config_lint::lint;
use std::path::PathBuf;

fn repo_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Unknown-key warnings only: the other lints depend on the combination of settings
fn problems(text: &str) -> Vec<String> {
    match lint(text) {
        Ok(w) => w
            .into_iter()
            .filter(|w| w.message.starts_with("Unknown key"))
            .map(|w| w.key)
            .collect(),
        Err(e) => vec![format!("does not load: {e}")],
    }
}

#[test]
fn example_config_files_load_clean() {
    for name in ["config.example.toml", "example-config.toml"] {
        let found = problems(&repo_file(name));
        assert!(found.is_empty(), "{name}: {found:?}");
    }
}

#[test]
fn readme_toml_blocks_load_clean() {
    let readme = repo_file("README.md");
    let mut bad = Vec::new();
    let mut block: Option<(usize, String)> = None;
    for (n, line) in readme.lines().enumerate() {
        match (&mut block, line.trim_end()) {
            (None, "```toml") => block = Some((n + 1, String::new())),
            (Some((start, text)), "```") => {
                let found = problems(text);
                if !found.is_empty() {
                    bad.push(format!("README.md line {start}: {found:?}"));
                }
                block = None;
            }
            (Some((_, text)), l) => {
                text.push_str(l);
                text.push('\n');
            }
            _ => {}
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
