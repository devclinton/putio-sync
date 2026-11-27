//! `putio-sync crash-report list|show|open` (feature 058)
//!
//! Reads the crash files next to the config file. Nothing is sent: `open` prints the
//! prefilled GitHub issue link and opens it in the browser for the person to review.

use putio_sync_core::crash::{self, CrashReport};
use putio_sync_core::t;
use std::io::Write;
use std::path::Path;

/// What to do
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashReportCommands {
    /// List the reports
    List {
        /// JSON instead of lines
        json: bool,
    },
    /// Print one report
    Show {
        /// Report id
        id: String,
    },
    /// Print and open the issue link
    Open {
        /// Report id
        id: String,
        /// Only print
        no_browser: bool,
    },
}

/// Run the command, writing to `out`. Exit code: 0 done, 1 no such report or no folder.
pub fn execute(command: &CrashReportCommands, config: Option<&Path>, out: &mut impl Write) -> i32 {
    let Some(dir) = crash::dir_for(config) else {
        eprintln!("{}", t!("Could not find the config folder"));
        return 1;
    };
    match command {
        CrashReportCommands::List { json } => list(&crash::list(&dir), *json, out),
        CrashReportCommands::Show { id } => match find(&dir, id) {
            Some(report) => {
                let _ = write!(out, "{}", report.to_text());
                0
            }
            None => missing(id),
        },
        CrashReportCommands::Open { id, no_browser } => match find(&dir, id) {
            Some(report) => open(&report, *no_browser, out, |url| {
                open::that(url).map_err(|e| e.to_string())
            }),
            None => missing(id),
        },
    }
}

fn find(dir: &Path, id: &str) -> Option<CrashReport> {
    crash::get(dir, id)
}

fn missing(id: &str) -> i32 {
    eprintln!(
        "{}",
        t!(
            "No crash report {0}. `putio-sync crash-report list` shows them.",
            format!("{:?}", id)
        )
    );
    1
}

fn list(reports: &[CrashReport], json: bool, out: &mut impl Write) -> i32 {
    if json {
        let rows: Vec<_> = reports
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id, "time": r.time, "kind": r.kind, "version": r.version,
                    "signature": r.signature(), "dismissed": r.dismissed,
                })
            })
            .collect();
        let _ = writeln!(out, "{}", serde_json::Value::Array(rows));
    } else if reports.is_empty() {
        let _ = writeln!(out, "{}", t!("No crash reports."));
    } else {
        for r in reports {
            let _ = writeln!(
                out,
                "{}  {}  {}  {}{}",
                r.id,
                r.time,
                r.version,
                r.signature(),
                if r.dismissed { "  (dismissed)" } else { "" }
            );
        }
    }
    0
}

/// Print the links and, unless `no_browser`, hand the issue link to `opener`
fn open(
    report: &CrashReport,
    no_browser: bool,
    out: &mut impl Write,
    opener: impl FnOnce(&str) -> Result<(), String>,
) -> i32 {
    let issue = report.issue();
    let _ = writeln!(
        out,
        "{}",
        t!(
            "Check the report first: putio-sync crash-report show {0}",
            report.id
        )
    );
    let _ = writeln!(
        out,
        "{}",
        t!("Search existing issues: {0}", issue.search_url)
    );
    let _ = writeln!(out, "{}", t!("Report on GitHub: {0}", issue.url));
    if issue.truncated {
        let _ = writeln!(out, "{}", t!("The report was cut to fit the link: paste the output of `crash-report show {0}` into the issue.", report.id));
    }
    if !no_browser {
        if let Err(e) = opener(&issue.url) {
            eprintln!(
                "{}",
                t!(
                    "Could not open the browser ({0}): open the link above yourself.",
                    e
                )
            );
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn report(dir: &Path) -> CrashReport {
        crash::write_panic(
            dir,
            Instant::now(),
            &crash::PanicDetails {
                thread: "main".into(),
                message: "boom".into(),
                location: None,
            },
            "   0: putio_sync_cli::main\n",
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn list_show_and_open_use_the_same_report() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let r = report(&dir.path().join(crash::DIR_NAME));

        let mut out = Vec::new();
        assert_eq!(
            execute(
                &CrashReportCommands::List { json: false },
                Some(&config),
                &mut out
            ),
            0
        );
        assert!(String::from_utf8(out).unwrap().contains(&r.id));

        let mut out = Vec::new();
        let show = CrashReportCommands::Show { id: r.id.clone() };
        assert_eq!(execute(&show, Some(&config), &mut out), 0);
        assert!(String::from_utf8(out).unwrap().contains("Message: boom"));

        let mut out = Vec::new();
        let open = CrashReportCommands::Open {
            id: r.id.clone(),
            no_browser: true,
        };
        assert_eq!(execute(&open, Some(&config), &mut out), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("https://github.com/devclinton/putio-sync/issues/new?"));
        assert!(text.contains("Search existing issues"));
    }

    #[test]
    fn open_calls_the_browser_helper_unless_told_not_to() {
        let dir = tempfile::tempdir().unwrap();
        let r = report(dir.path());
        let mut opened = None;
        open(&r, false, &mut Vec::new(), |url| {
            opened = Some(url.to_string());
            Ok(())
        });
        assert_eq!(opened.as_deref(), Some(r.issue().url.as_str()));
        open(&r, true, &mut Vec::new(), |_| panic!("opened"));
    }

    #[test]
    fn an_unknown_id_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let show = CrashReportCommands::Show {
            id: "20200101T000000000-000".into(),
        };
        assert_eq!(execute(&show, Some(&config), &mut Vec::new()), 1);
    }
}
