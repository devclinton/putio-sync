//! Progress bar implementations for download tracking
//!
//! Provides multiple progress display styles and handles terminal detection
//! to automatically disable progress bars in non-interactive environments.

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle as IndicatifStyle};
use putio_sync_core::config::ProgressStyle;
use putio_sync_download::ProgressCallback;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Progress display manager
pub struct ProgressDisplay {
    /// Multi-progress for managing multiple progress bars
    multi: MultiProgress,
    /// Overall progress bar
    overall_bar: ProgressBar,
    /// Individual file progress bars
    file_bars: Arc<Mutex<HashMap<i64, ProgressBar>>>,
    /// Progress style
    style: ProgressStyle,
    /// Total number of files
    total_files: u64,
    /// Total size in bytes
    total_size: u64,
    /// Files completed
    files_completed: Arc<Mutex<u64>>,
    /// Bytes downloaded
    bytes_downloaded: Arc<Mutex<u64>>,
    /// Start time
    start_time: Instant,
}

impl ProgressDisplay {
    /// Create a new progress display
    ///
    /// # Arguments
    ///
    /// * `style` - Progress display style
    /// * `total_files` - Total number of files to download
    /// * `total_size` - Total size in bytes
    ///
    /// # Returns
    ///
    /// Progress display instance
    pub fn new(style: ProgressStyle, total_files: u64, total_size: u64) -> Self {
        let multi = MultiProgress::new();

        // Detect if we're in an interactive terminal
        if !atty::is(atty::Stream::Stdout) || style == ProgressStyle::None {
            multi.set_draw_target(ProgressDrawTarget::hidden());
        }

        let overall_bar = create_overall_progress_bar(total_files, total_size);

        // Only add overall bar for certain styles
        let overall_bar = match style {
            ProgressStyle::Overall | ProgressStyle::PerFile | ProgressStyle::Rotating => {
                multi.add(overall_bar.clone());
                overall_bar
            }
            _ => overall_bar,
        };

        Self {
            multi,
            overall_bar,
            file_bars: Arc::new(Mutex::new(HashMap::new())),
            style,
            total_files,
            total_size,
            files_completed: Arc::new(Mutex::new(0)),
            bytes_downloaded: Arc::new(Mutex::new(0)),
            start_time: Instant::now(),
        }
    }

    /// Update progress for a file
    ///
    /// # Arguments
    ///
    /// * `file_id` - File ID
    /// * `bytes_downloaded` - Bytes downloaded for this file
    pub fn update_file_progress(&self, file_id: i64, bytes_downloaded: u64) {
        // Update individual file progress bar
        if matches!(
            self.style,
            ProgressStyle::Individual | ProgressStyle::PerFile | ProgressStyle::Rotating
        ) {
            let bars = self.file_bars.lock().unwrap();
            if let Some(bar) = bars.get(&file_id) {
                bar.set_position(bytes_downloaded);
            }
        }

        // Update overall progress
        self.update_overall_progress();
    }

    /// Mark file as started
    ///
    /// # Arguments
    ///
    /// * `file_id` - File ID
    /// * `filename` - Filename
    /// * `file_size` - File size in bytes
    pub fn file_started(&self, file_id: i64, filename: &str, file_size: u64) {
        // Create file progress bar for certain styles
        if matches!(
            self.style,
            ProgressStyle::Individual | ProgressStyle::PerFile | ProgressStyle::Rotating
        ) {
            let bar = create_file_progress_bar(filename, file_size);
            let bar = self.multi.add(bar);

            let mut bars = self.file_bars.lock().unwrap();
            bars.insert(file_id, bar);
        }
    }

    /// Mark file as completed
    ///
    /// # Arguments
    ///
    /// * `file_id` - File ID
    /// * `success` - Whether the download was successful
    pub fn file_completed(&self, file_id: i64, success: bool) {
        // Update file count
        {
            let mut completed = self.files_completed.lock().unwrap();
            *completed += 1;
        }

        // Finish and remove file progress bar
        if matches!(
            self.style,
            ProgressStyle::Individual | ProgressStyle::PerFile | ProgressStyle::Rotating
        ) {
            let mut bars = self.file_bars.lock().unwrap();
            if let Some(bar) = bars.remove(&file_id) {
                if success {
                    bar.finish_with_message("✓");
                } else {
                    bar.finish_with_message("✗");
                }
            }
        }

        // Update overall progress
        self.update_overall_progress();
    }

    /// Update overall progress bar
    fn update_overall_progress(&self) {
        if matches!(
            self.style,
            ProgressStyle::Overall | ProgressStyle::PerFile | ProgressStyle::Rotating
        ) {
            let completed = *self.files_completed.lock().unwrap();
            let bytes = *self.bytes_downloaded.lock().unwrap();

            self.overall_bar.set_position(bytes);

            let elapsed = self.start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                bytes as f64 / elapsed
            } else {
                0.0
            };

            let eta = if speed > 0.0 {
                ((self.total_size - bytes) as f64 / speed) as u64
            } else {
                0
            };

            self.overall_bar.set_message(format!(
                "{}/{} files | {} | ETA: {}",
                completed,
                self.total_files,
                format_speed(speed),
                format_duration(eta)
            ));
        }
    }

    /// Finish all progress bars
    pub fn finish(&self) {
        self.overall_bar.finish_with_message("Download complete!");

        // Clear all file bars
        let bars = self.file_bars.lock().unwrap();
        for bar in bars.values() {
            bar.finish_and_clear();
        }
    }

    /// Create a progress callback for the download manager
    ///
    /// # Returns
    ///
    /// Arc-wrapped progress callback
    pub fn create_callback(self: Arc<Self>) -> Arc<dyn ProgressCallback> {
        Arc::new(ProgressCallbackImpl {
            display: self.clone(),
        })
    }
}

/// Implementation of ProgressCallback for integration with core library
struct ProgressCallbackImpl {
    display: Arc<ProgressDisplay>,
}

impl ProgressCallback for ProgressCallbackImpl {
    fn on_file_start(&self, file_id: i64, filename: &str, file_size: u64) {
        self.display.file_started(file_id, filename, file_size);
    }

    fn on_file_progress(&self, progress: &putio_sync_download::DownloadProgress) {
        self.display
            .update_file_progress(progress.file_id, progress.bytes_downloaded);

        // Update total bytes downloaded
        let mut total = self.display.bytes_downloaded.lock().unwrap();
        *total = progress.bytes_downloaded;
    }

    fn on_file_complete(&self, file_id: i64, _filename: &str, success: bool) {
        self.display.file_completed(file_id, success);
    }

    fn on_overall_progress(&self, _progress: &putio_sync_download::OverallProgress) {
        // Already handled in update_file_progress
    }
}

/// Create overall progress bar
fn create_overall_progress_bar(total_files: u64, total_size: u64) -> ProgressBar {
    let bar = ProgressBar::new(total_size);
    bar.set_style(
        IndicatifStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("=> "),
    );
    bar.set_message(format!("0/{} files", total_files));
    bar
}

/// Create file progress bar
fn create_file_progress_bar(filename: &str, file_size: u64) -> ProgressBar {
    let bar = ProgressBar::new(file_size);

    // Truncate filename if too long
    let display_name = if filename.len() > 40 {
        format!("{}...", &filename[..37])
    } else {
        filename.to_string()
    };

    bar.set_style(
        IndicatifStyle::default_bar()
            .template(&format!("  {{spinner:.green}} {} [{{bar:30.cyan/blue}}] {{bytes}}/{{total_bytes}} {{msg}}", display_name))
            .expect("Invalid progress bar template")
            .progress_chars("=> "),
    );
    bar
}

/// Format speed in bytes/sec to human-readable format
fn format_speed(bytes_per_sec: f64) -> String {
    const UNITS: &[&str] = &["B/s", "KB/s", "MB/s", "GB/s"];
    let mut size = bytes_per_sec;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_idx])
}

/// Format duration in seconds to human-readable format
fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(512.0), "512.00 B/s");
        assert_eq!(format_speed(1024.0), "1.00 KB/s");
        assert_eq!(format_speed(1536.0), "1.50 KB/s");
        assert_eq!(format_speed(1048576.0), "1.00 MB/s");
        assert_eq!(format_speed(107374182.4), "102.40 MB/s");
        assert_eq!(format_speed(1073741824.0), "1.00 GB/s");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(7200), "2h 0m");
    }

    #[test]
    fn test_progress_display_creation() {
        let display = ProgressDisplay::new(ProgressStyle::PerFile, 10, 1000000);
        assert_eq!(display.total_files, 10);
        assert_eq!(display.total_size, 1000000);
        assert_eq!(*display.files_completed.lock().unwrap(), 0);
        assert_eq!(*display.bytes_downloaded.lock().unwrap(), 0);
    }

    #[test]
    fn test_file_tracking() {
        let display = ProgressDisplay::new(ProgressStyle::Individual, 1, 1000);
        display.file_started(1, "test.txt", 1000);

        {
            let bars = display.file_bars.lock().unwrap();
            assert!(bars.contains_key(&1));
        }

        display.file_completed(1, true);

        {
            let completed = display.files_completed.lock().unwrap();
            assert_eq!(*completed, 1);
        }

        {
            let bars = display.file_bars.lock().unwrap();
            assert!(!bars.contains_key(&1));
        }
    }
}
