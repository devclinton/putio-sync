//! `files clean-empty`: remove empty folders from Put.io

use anyhow::{Context, Result};
use putio_sync_client::{EmptyFolderReport, FailedStep, PutioApiClient};
use putio_sync_core::cleanup::{CleanupTrigger, EmptyFolderCleanup};
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_core::t;

use std::path::PathBuf;
use std::sync::Arc;

/// Execute the clean-empty command
///
/// Prints what a run would remove, asks unless `yes`, then removes it. With `dry_run`
/// nothing is deleted. Fails when the run fails or any folder could not be listed or
/// deleted.
pub async fn execute_clean_empty(
    dry_run: bool,
    yes: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    let config = load_config(config_path)?;
    let client = Arc::new(build_client(&config)?);
    let cleanup = EmptyFolderCleanup::from_config(client, &config);

    let preview = cleanup.run(CleanupTrigger::Manual, true).await?;
    print_report(&preview);
    if dry_run {
        println!("\n{}", t!("Dry run mode - no folders were deleted"));
        return check_failures(&preview);
    }
    if preview.removed.is_empty() {
        return check_failures(&preview);
    }

    if !yes && !confirm(preview.removed.len())? {
        println!("{}", t!("Cleanup cancelled"));
        return Ok(());
    }

    println!("\n{}", t!("Deleting empty folders..."));
    let report = cleanup.run(CleanupTrigger::Manual, false).await?;
    print_report(&report);
    check_failures(&report)
}

fn print_report(report: &EmptyFolderReport) {
    if report.removed.is_empty() {
        println!("{}", t!("No empty folders to remove"));
    } else {
        let heading = if report.dry_run {
            "Empty folders to remove:"
        } else {
            "Removed empty folders:"
        };
        println!("\n{heading}");
        println!(
            "{} {} {}",
            crate::output::pad(&t!("ID"), 12),
            crate::output::pad(&t!("Age"), 10),
            t!("Path")
        );
        println!("{}", "-".repeat(70));
        for folder in &report.removed {
            println!(
                "{:<12} {:<10} {}",
                folder.id,
                format_age(folder.age_hours),
                folder.path
            );
        }
        println!("{}", "-".repeat(70));
    }
    // The preview says what is kept and why (feature 021); the real run would repeat it
    if report.dry_run && !report.kept.is_empty() {
        println!("\n{}", t!("Empty folders kept:"));
        for folder in &report.kept {
            println!("{:<12} {} ({})", folder.id, folder.path, folder.reason);
        }
    }
    for failure in &report.failures {
        let step = match failure.step {
            FailedStep::List => "list",
            FailedStep::Delete => "delete",
        };
        eprintln!(
            "{}",
            t!(
                "Failed to {0} {1} ({2}): {3}",
                step,
                failure.path,
                failure.id,
                failure.error
            )
        );
    }
    println!("{}", t!("Scanned {0} folders: {1} {2}, kept {3} protected, {4} too new, {5} by keep settings, {6} failed", report.scanned, if report.dry_run {
            "would remove"
        } else {
            "removed"
        }, report.removed.len(), report.kept_protected, report.kept_too_new, report.kept_by_user, report.failures.len()));
}

fn check_failures(report: &EmptyFolderReport) -> Result<()> {
    if report.failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} folder(s) could not be cleaned up",
            report.failures.len()
        )
    }
}

fn format_age(hours: u64) -> String {
    if hours >= 48 {
        format!("{}d", hours / 24)
    } else {
        format!("{}h", hours)
    }
}

fn confirm(count: usize) -> Result<bool> {
    println!();
    Ok(crate::ui::confirm(&t!(
        "Delete {0} empty folder(s) from Put.io?",
        count
    ))?)
}

pub(crate) fn build_client(config: &Config) -> Result<PutioApiClient> {
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

pub(crate) fn load_config(config_path: Option<PathBuf>) -> Result<Config> {
    let path = config_path.unwrap_or_else(|| {
        Config::default_path().expect("Failed to determine default config path")
    });

    Config::load(&path).context("Failed to load configuration. Run 'putio-sync login' first.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_in_hours_then_days() {
        assert_eq!(format_age(5), "5h");
        assert_eq!(format_age(47), "47h");
        assert_eq!(format_age(72), "3d");
    }
}
