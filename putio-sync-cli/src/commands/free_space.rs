//! `files free-space`: delete the oldest Put.io files that are already downloaded until
//! `[storage] target_free_percent` is free (feature 033)

use super::clean_empty::{build_client, load_config};
use crate::output::format_bytes;
use crate::services::Services;
use anyhow::{Context, Result};
use putio_sync_core::config::Config;
use putio_sync_core::t;
use putio_sync_db::Database;
use putio_sync_storage::{FreeResult, Plan, StorageService, Trigger};

use std::path::PathBuf;
use std::sync::Arc;

/// Execute the free-space command
///
/// Prints the preview, asks unless `yes`, then deletes exactly the previewed files. With
/// `dry_run` nothing is deleted. Fails when any file could not be deleted.
pub async fn execute_free_space(
    dry_run: bool,
    yes: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    let config = load_config(config_path.clone())?;
    // Low-storage events go to the hooks and plugins
    let resolved = config_path
        .clone()
        .or_else(|| Config::default_path().ok())
        .unwrap_or_default();
    let services = Services::start(&config, &resolved);
    let result = free_space(&config, config_path, dry_run, yes).await;
    services.finish().await;
    result
}

async fn free_space(
    config: &Config,
    config_path: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let client = Arc::new(build_client(config)?);
    // The download records decide what is safe to delete; never create the database here
    let db_path = putio_sync_core::database::prepare(config, config_path.as_deref()).await;
    anyhow::ensure!(
        db_path.is_file(),
        "No download database at {}; freeing space needs the download records",
        db_path.display()
    );
    let db = Arc::new(
        Database::new(&db_path)
            .await
            .with_context(|| format!("Could not open the database {}", db_path.display()))?,
    );
    let storage = StorageService::from_config(client, Some(db), config);

    let plan = storage.plan().await?;
    print_plan(&plan);
    if dry_run {
        println!("\n{}", t!("Dry run mode - no files were deleted"));
        return Ok(());
    }
    if plan.files.is_empty() {
        return Ok(());
    }
    if !yes && !confirm(plan.files.len(), plan.total_bytes)? {
        println!("{}", t!("Free-up cancelled"));
        return Ok(());
    }

    println!("\n{}", t!("Deleting files..."));
    let ids: Vec<i64> = plan.files.iter().map(|f| f.id).collect();
    let result = storage.free(Trigger::Manual, &plan.id, &ids).await?;
    print_result(&result);
    if result.failed.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{} file(s) could not be deleted", result.failed.len())
    }
}

fn print_plan(plan: &Plan) {
    if plan.needed_bytes == 0 {
        println!(
            "{}",
            t!(
                "{0} free; the target ({1}) is already met",
                format_bytes(plan.free_before),
                format_bytes(plan.target_bytes)
            )
        );
        return;
    }
    if plan.files.is_empty() {
        println!("{}", t!("No files are safe to delete"));
    } else {
        println!("\n{}", t!("Files to delete, oldest first:"));
        println!(
            "{} {} {} {}",
            crate::output::pad(&t!("Size"), 10),
            crate::output::pad(&t!("ID"), 12),
            crate::output::pad(&t!("Added"), 20),
            t!("Path / local copy")
        );
        println!("{}", "-".repeat(90));
        for f in &plan.files {
            println!(
                "{:<10} {:<12} {:<20} {}",
                format_bytes(f.size),
                f.id,
                f.created_at.as_deref().unwrap_or("-"),
                f.path
            );
            println!("{:<44} {}", "", f.local_path);
        }
        println!("{}", "-".repeat(90));
    }
    println!(
        "{}",
        t!(
            "{0} files, {1}: free {2} → {3} (target {4})",
            plan.files.len(),
            format_bytes(plan.total_bytes),
            format_bytes(plan.free_before),
            format_bytes(plan.free_after),
            format_bytes(plan.target_bytes)
        )
    );
    if plan.shortfall_bytes > 0 {
        println!(
            "{}",
            t!(
                "Not enough: {0} short of the target",
                format_bytes(plan.shortfall_bytes)
            )
        );
    }
    if !plan.complete {
        println!(
            "{}",
            t!("The account could not be listed completely; some files were not considered")
        );
    }
}

fn print_result(result: &FreeResult) {
    for f in &result.deleted {
        println!("{}", t!("Deleted {0} ({1})", f.path, format_bytes(f.size)));
    }
    for s in &result.skipped {
        println!("{}", t!("Skipped {0} ({1}): {2}", s.path, s.id, s.reason));
    }
    for f in &result.failed {
        eprintln!(
            "{}",
            t!("Failed to delete {0} ({1}): {2}", f.path, f.id, f.error)
        );
    }
    println!(
        "{}",
        t!(
            "Deleted {0} files ({1}), skipped {2}, failed {3}{4}",
            result.deleted.len(),
            format_bytes(result.bytes_deleted),
            result.skipped.len(),
            result.failed.len(),
            result
                .free_after
                .map(|b| format!("; {} free now", format_bytes(b)))
                .unwrap_or_default()
        )
    );
}

fn confirm(count: usize, bytes: i64) -> Result<bool> {
    println!();
    Ok(crate::ui::confirm(&t!(
        "Delete {0} files ({1}) from Put.io?",
        count,
        format_bytes(bytes)
    ))?)
}
