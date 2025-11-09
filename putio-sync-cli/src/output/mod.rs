//! Output formatters for list command

mod csv;
mod json;
mod table;
mod tree;

pub use csv::format_csv;
pub use json::format_json;
pub use table::format_table;
pub use tree::format_tree;

use putio_sync_core::api::FileEntry;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::str::FromStr;

/// Output format options
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    /// Table format (default)
    Table,
    /// JSON format
    Json,
    /// CSV format
    Csv,
    /// Tree format
    Tree,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "tree" => Ok(Self::Tree),
            _ => Err(format!("Invalid output format: {}", s)),
        }
    }
}

/// Format and output file listing
pub fn format_and_output(
    files: &[FileEntry],
    format: OutputFormat,
    output_file: Option<&Path>,
) -> io::Result<()> {
    let output = match format {
        OutputFormat::Table => format_table(files),
        OutputFormat::Json => format_json(files),
        OutputFormat::Csv => format_csv(files),
        OutputFormat::Tree => format_tree(files),
    };

    if let Some(file_path) = output_file {
        let mut file = File::create(file_path)?;
        file.write_all(output.as_bytes())?;
    } else {
        print!("{}", output);
    }

    Ok(())
}

/// Format bytes into human-readable size
pub fn format_bytes(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size as i64, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

/// Format timestamp
pub fn format_timestamp(timestamp: &Option<String>) -> String {
    timestamp
        .as_ref()
        .map(|s| {
            // Parse ISO 8601 timestamp and format as date
            s.split('T').next().unwrap_or(s).to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}
