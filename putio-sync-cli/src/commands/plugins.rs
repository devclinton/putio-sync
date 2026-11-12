//! `putio-sync plugins check` (feature 059): check a plugin folder the way a reload loads
//! it, for plugin authors and the plugin registry's checks. Needs no config or token.

use std::io::Write;
use std::path::Path;

use putio_sync_plugins::source;

/// Check `folder` and write the report to `out`. Exit code: 0 no problems, 1 problems,
/// 2 not a folder.
pub fn execute_check(folder: &Path, json: bool, out: &mut impl Write) -> i32 {
    if !folder.is_dir() {
        eprintln!("{}: not a folder", folder.display());
        return 2;
    }
    let (manifest, problems) = source::check_folder(folder);
    let name = manifest
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let version = manifest
        .as_ref()
        .map(|m| m.version.clone())
        .unwrap_or_default();
    if json {
        let report = serde_json::json!({
            "name": name,
            "version": version,
            "problems": problems,
        });
        let _ = writeln!(out, "{}", report);
    } else {
        for p in &problems {
            match p.line {
                Some(line) => {
                    let _ = writeln!(out, "{}:{}: {}", p.file, line, p.message);
                }
                None => {
                    let _ = writeln!(out, "{}: {}", p.file, p.message);
                }
            }
        }
        let label = if name.is_empty() {
            folder.display().to_string()
        } else {
            format!("{} {}", name, version)
        };
        let _ = match problems.len() {
            0 => writeln!(out, "{}: ok", label),
            1 => writeln!(out, "{}: 1 problem", label),
            n => writeln!(out, "{}: {} problems", label, n),
        };
    }
    i32::from(!problems.is_empty())
}
