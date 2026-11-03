//! Unit tests for the web UI's plain modules (feature 045): runs `node --test` over
//! `tests/ui/`. Without `node` on `PATH` it says so and passes, so a machine without
//! Node can still run the rest of the suite.

use std::path::Path;
use std::process::Command;

#[test]
fn ui_modules_pass_their_node_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let output = match Command::new("node")
        .arg("--test")
        .arg(dir.join("*.test.mjs"))
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            eprintln!("skipping the UI module tests: can't run node ({e})");
            return;
        }
    };
    assert!(
        output.status.success(),
        "node --test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
