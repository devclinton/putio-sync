//! `doctor`: run the health checks, or preview and apply one fix (feature 010)
//!
//! Exit codes follow `specs/010-putio-doctor/contracts/cli.md`: 0 ok, 1 warning,
//! 2 problem, 3 the doctor could not run.

use anyhow::{Context, Result};
use putio_sync_client::PutioApiClient;
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_db::Database;
use putio_sync_doctor::report::FixInfo;
use putio_sync_doctor::{
    Area, CheckResult, Doctor, FixId, FixOutcome, FixPreview, FixRequest, Item, Report, Status,
};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// The doctor could not run
pub const EXIT_FAILED: i32 = 3;
/// Items printed per check in text mode
const SHOWN_ITEMS: usize = 10;

/// Run the command and return its exit code
pub async fn execute_doctor(
    json: bool,
    fix: Option<String>,
    dry_run: bool,
    yes: bool,
    config_path: Option<PathBuf>,
) -> i32 {
    let doctor = match build(config_path).await {
        Ok(doctor) => doctor,
        Err(e) => {
            eprintln!("The doctor could not run: {e:#}");
            return EXIT_FAILED;
        }
    };
    let mut out = io::stdout().lock();
    let result = match fix {
        None => report(&doctor, json, &mut out).await,
        Some(id) => {
            let mut input = io::stdin().lock();
            apply_fix(&doctor, &id, json, dry_run, yes, &mut input, &mut out).await
        }
    };
    result.unwrap_or_else(|e| {
        eprintln!("The doctor could not write its output: {e}");
        EXIT_FAILED
    })
}

/// A doctor for this machine's config. The database is opened only if it exists, so a
/// run never creates one.
async fn build(config_path: Option<PathBuf>) -> Result<Doctor> {
    let path = match config_path {
        Some(path) => path,
        None => Config::default_path().context("Could not find the config file")?,
    };
    let config =
        Config::load(&path).with_context(|| format!("Could not load {}", path.display()))?;
    let client = Arc::new(build_client(&config)?);
    // Not `prepare`: the doctor only looks, and reports a database left in the download folder
    let db_path = putio_sync_core::database::path_for(&config, Some(&path));
    let db = if db_path.is_file() {
        Some(Arc::new(Database::new(&db_path).await.with_context(
            || format!("Could not open the database {}", db_path.display()),
        )?))
    } else {
        None
    };
    let cleanup = Arc::new(EmptyFolderCleanup::from_config(client.clone(), &config));
    Ok(Doctor::new(config, client, db, cleanup).with_config_path(Some(path)))
}

fn build_client(config: &Config) -> Result<PutioApiClient> {
    let auth = config
        .auth
        .clone()
        .context("No authentication configured. Run 'putio-sync login' first.")?;
    Ok(match auth.auth_method {
        AuthMethod::Token => {
            let token = auth
                .token
                .context("OAuth token not configured. Run 'putio-sync login' first.")?;
            PutioApiClient::new(token)
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .context("Username not configured for app password authentication")?;
            let password = auth.password.context("App password not configured")?;
            PutioApiClient::with_credentials(username, password)
        }
    })
}

/// Exit code for a report
pub fn exit_code(overall: Status) -> i32 {
    match overall {
        Status::Ok | Status::Skipped => 0,
        Status::Warning => 1,
        Status::Problem => 2,
    }
}

/// Run every check and print the report
pub async fn report(doctor: &Doctor, json: bool, out: &mut impl Write) -> io::Result<i32> {
    let report = doctor.run().await;
    if json {
        serde_json::to_writer_pretty(&mut *out, &report)?;
        writeln!(out)?;
    } else {
        print_report(&report, out)?;
    }
    Ok(exit_code(report.overall))
}

fn mark(status: Status) -> &'static str {
    match status {
        Status::Ok => "✓",
        Status::Warning => "!",
        Status::Problem => "✗",
        Status::Skipped => "-",
    }
}

fn item_line(item: &Item) -> String {
    let mut line = format!("- {}", item.label);
    if let Some(detail) = &item.detail {
        line.push_str(&format!(" ({detail})"));
    }
    if let Some(fix) = item.fix {
        line.push_str(&format!("  [fix: {fix}]"));
    }
    line
}

/// Print one check, with its title padded to `width`
fn print_check(check: &CheckResult, width: usize, out: &mut impl Write) -> io::Result<()> {
    let pad = " ".repeat(width + 5);
    writeln!(
        out,
        "  {} {:<width$} {}",
        mark(check.status),
        check.title,
        check.summary
    )?;
    if let Some(suggestion) = &check.suggestion {
        writeln!(out, "{pad}→ {suggestion}")?;
    }
    for item in check.items.iter().take(SHOWN_ITEMS) {
        writeln!(out, "{pad}{}", item_line(item))?;
    }
    let shown = check.items.len().min(SHOWN_ITEMS) as u64;
    if check.item_count > shown {
        writeln!(out, "{pad}… and {} more", check.item_count - shown)?;
    }
    Ok(())
}

fn print_report(report: &Report, out: &mut impl Write) -> io::Result<()> {
    let width = report
        .checks
        .iter()
        .map(|c| c.title.chars().count())
        .max()
        .unwrap_or(0);
    for area in Area::ALL {
        let checks: Vec<&CheckResult> = report.checks.iter().filter(|c| c.area == area).collect();
        if checks.is_empty() {
            continue;
        }
        writeln!(out, "{}", area.title())?;
        for check in checks {
            print_check(check, width, out)?;
        }
    }
    let c = report.counts;
    writeln!(
        out,
        "\nSummary: {} problem{}, {} warning{}, {} ok, {} skipped (took {:.1}s)",
        c.problem,
        if c.problem == 1 { "" } else { "s" },
        c.warning,
        if c.warning == 1 { "" } else { "s" },
        c.ok,
        c.skipped,
        report.duration_ms as f64 / 1000.0
    )?;
    let fixes: Vec<&FixInfo> = report.checks.iter().flat_map(|c| &c.fixes).collect();
    if !fixes.is_empty() {
        let ids: Vec<&str> = fixes.iter().map(|f| f.id.as_str()).collect();
        writeln!(
            out,
            "Fixes available: {} — run `putio-sync doctor --fix <id>`",
            ids.join(", ")
        )?;
    }
    Ok(())
}

fn print_preview(preview: &FixPreview, out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "{} ({}) would act on {} item{}:",
        preview.fix.label(),
        preview.fix,
        preview.item_count,
        if preview.item_count == 1 { "" } else { "s" }
    )?;
    // Every item, so a confirmation covers exactly what was shown
    for item in &preview.items {
        writeln!(out, "  {}", item_line(item))?;
    }
    Ok(())
}

fn print_outcome(outcome: &FixOutcome, out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "\nDone: {}, skipped: {}, failed: {}",
        outcome.done.len(),
        outcome.skipped.len(),
        outcome.failed.len()
    )?;
    for s in &outcome.skipped {
        writeln!(out, "  skipped {}: {}", s.item, s.reason)?;
    }
    for f in &outcome.failed {
        writeln!(out, "  failed {}: {}", f.item, f.error)?;
    }
    writeln!(out, "\nChecked again:")?;
    print_check(&outcome.check, outcome.check.title.chars().count(), out)
}

fn confirm(
    preview: &FixPreview,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<bool> {
    write!(
        out,
        "\nApply {} to {} item(s)? [y/N] ",
        preview.fix.label(),
        preview.item_count
    )?;
    out.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    let answer = answer.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// Preview fix `id`, ask if it deletes, apply it and print what happened
pub async fn apply_fix(
    doctor: &Doctor,
    id: &str,
    json: bool,
    dry_run: bool,
    yes: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<i32> {
    let Ok(fix) = id.parse::<FixId>() else {
        let ids: Vec<&str> = FixId::ALL.iter().map(|f| f.as_str()).collect();
        eprintln!("Unknown fix {id:?}. Valid fixes: {}", ids.join(", "));
        return Ok(EXIT_FAILED);
    };
    if json && fix.destructive() && !dry_run && !yes {
        eprintln!("{fix} deletes data and --json can't ask first; add --yes or --dry-run.");
        return Ok(EXIT_FAILED);
    }
    let preview = doctor.preview(fix).await;
    if dry_run || preview.items.is_empty() {
        if json {
            serde_json::to_writer_pretty(&mut *out, &preview)?;
            writeln!(out)?;
        } else if preview.items.is_empty() {
            writeln!(out, "Nothing to fix.")?;
        } else {
            print_preview(&preview, out)?;
            writeln!(out, "\nDry run: nothing was changed.")?;
        }
        return Ok(0);
    }
    if !json {
        print_preview(&preview, out)?;
    }
    if fix.destructive() && !yes && !confirm(&preview, input, out)? {
        writeln!(out, "Cancelled; nothing was changed.")?;
        return Ok(0);
    }
    let request = FixRequest {
        items: preview.items.iter().map(|i| i.key.clone()).collect(),
        confirm: true,
    };
    let outcome = match doctor.apply(fix, &request).await {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("The fix was refused: {e}");
            return Ok(EXIT_FAILED);
        }
    };
    if json {
        serde_json::to_writer_pretty(&mut *out, &outcome)?;
        writeln!(out)?;
    } else {
        print_outcome(&outcome, out)?;
    }
    Ok(if outcome.failed.is_empty() { 0 } else { 1 })
}
