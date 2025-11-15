//! Progress tracking for downloads

use std::time::{Duration, Instant};

/// Progress update for a download
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// File ID
    pub file_id: i64,
    /// Filename
    pub filename: String,
    /// Bytes downloaded so far
    pub bytes_downloaded: u64,
    /// Total file size
    pub total_bytes: u64,
    /// Download speed in bytes per second
    pub speed_bps: f64,
    /// Estimated time remaining
    pub eta_seconds: Option<u64>,
    /// Whether the download is complete
    pub complete: bool,
}

impl DownloadProgress {
    /// Create a new progress tracker
    pub fn new(file_id: i64, filename: String, total_bytes: u64) -> Self {
        Self {
            file_id,
            filename,
            bytes_downloaded: 0,
            total_bytes,
            speed_bps: 0.0,
            eta_seconds: None,
            complete: false,
        }
    }

    /// Get progress as a percentage (0.0 to 100.0)
    pub fn percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.bytes_downloaded as f64 / self.total_bytes as f64) * 100.0
    }

    /// Check if download is complete
    pub fn is_complete(&self) -> bool {
        self.complete || self.bytes_downloaded >= self.total_bytes
    }
}

/// Overall progress for all downloads
#[derive(Debug, Clone)]
pub struct OverallProgress {
    /// Total number of files to download
    pub total_files: usize,
    /// Number of files completed
    pub completed_files: usize,
    /// Number of files currently downloading
    pub active_files: usize,
    /// Number of files pending
    pub pending_files: usize,
    /// Number of files failed
    pub failed_files: usize,
    /// Total bytes to download across all files
    pub total_bytes: u64,
    /// Total bytes downloaded across all files
    pub downloaded_bytes: u64,
    /// Overall download speed in bytes per second
    pub speed_bps: f64,
    /// Estimated time remaining in seconds
    pub eta_seconds: Option<u64>,
}

impl OverallProgress {
    /// Create a new overall progress tracker
    pub fn new(total_files: usize, total_bytes: u64) -> Self {
        Self {
            total_files,
            completed_files: 0,
            active_files: 0,
            pending_files: total_files,
            failed_files: 0,
            total_bytes,
            downloaded_bytes: 0,
            speed_bps: 0.0,
            eta_seconds: None,
        }
    }

    /// Get progress as a percentage (0.0 to 100.0)
    pub fn percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.downloaded_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    /// Check if all downloads are complete
    pub fn is_complete(&self) -> bool {
        self.completed_files + self.failed_files >= self.total_files
    }
}

/// Trait for receiving progress updates
pub trait ProgressCallback: Send + Sync {
    /// Called when a file's progress is updated
    fn on_file_progress(&self, progress: &DownloadProgress);

    /// Called when overall progress is updated
    fn on_overall_progress(&self, progress: &OverallProgress);

    /// Called when a file download starts
    fn on_file_start(&self, file_id: i64, filename: &str, size: u64);

    /// Called when a file download completes
    fn on_file_complete(&self, file_id: i64, filename: &str, success: bool);
}

/// A progress tracker that calculates speed and ETA
pub struct ProgressTracker {
    /// Bytes downloaded at start
    start_bytes: u64,
    /// Time when tracking started
    start_time: Instant,
    /// Bytes downloaded at last update
    last_bytes: u64,
    /// Time of last update
    last_update: Instant,
    /// Moving average window for speed calculation (in samples)
    speed_samples: Vec<f64>,
    /// Maximum number of speed samples to keep
    max_samples: usize,
}

impl ProgressTracker {
    /// Create a new progress tracker
    pub fn new(initial_bytes: u64) -> Self {
        let now = Instant::now();
        Self {
            start_bytes: initial_bytes,
            start_time: now,
            last_bytes: initial_bytes,
            last_update: now,
            speed_samples: Vec::new(),
            max_samples: 10,
        }
    }

    /// Update progress and calculate speed/ETA
    pub fn update(&mut self, current_bytes: u64, total_bytes: u64) -> (f64, Option<u64>) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);

        // Calculate instantaneous speed
        if elapsed > Duration::from_millis(100) {
            let bytes_diff = current_bytes.saturating_sub(self.last_bytes) as f64;
            let speed = bytes_diff / elapsed.as_secs_f64();

            // Add to samples
            self.speed_samples.push(speed);
            if self.speed_samples.len() > self.max_samples {
                self.speed_samples.remove(0);
            }

            self.last_bytes = current_bytes;
            self.last_update = now;
        }

        // Calculate average speed
        let avg_speed = if !self.speed_samples.is_empty() {
            self.speed_samples.iter().sum::<f64>() / self.speed_samples.len() as f64
        } else {
            0.0
        };

        // Calculate ETA
        let eta = if avg_speed > 0.0 && current_bytes < total_bytes {
            let remaining_bytes = total_bytes.saturating_sub(current_bytes) as f64;
            Some((remaining_bytes / avg_speed) as u64)
        } else {
            None
        };

        (avg_speed, eta)
    }

    /// Get overall statistics since start
    pub fn overall_stats(&self) -> (f64, Duration) {
        let elapsed = self.start_time.elapsed();
        let bytes_diff = self.last_bytes.saturating_sub(self.start_bytes) as f64;
        let avg_speed = if elapsed.as_secs_f64() > 0.0 {
            bytes_diff / elapsed.as_secs_f64()
        } else {
            0.0
        };

        (avg_speed, elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_progress_new() {
        let progress = DownloadProgress::new(123, "test.txt".to_string(), 1024);
        assert_eq!(progress.file_id, 123);
        assert_eq!(progress.filename, "test.txt");
        assert_eq!(progress.total_bytes, 1024);
        assert_eq!(progress.bytes_downloaded, 0);
        assert_eq!(progress.percentage(), 0.0);
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_download_progress_percentage() {
        let mut progress = DownloadProgress::new(123, "test.txt".to_string(), 1000);
        progress.bytes_downloaded = 250;
        assert_eq!(progress.percentage(), 25.0);

        progress.bytes_downloaded = 500;
        assert_eq!(progress.percentage(), 50.0);

        progress.bytes_downloaded = 1000;
        assert_eq!(progress.percentage(), 100.0);
    }

    #[test]
    fn test_download_progress_complete() {
        let mut progress = DownloadProgress::new(123, "test.txt".to_string(), 1000);
        assert!(!progress.is_complete());

        progress.bytes_downloaded = 1000;
        assert!(progress.is_complete());

        progress.complete = true;
        progress.bytes_downloaded = 500;
        assert!(progress.is_complete());
    }

    #[test]
    fn test_overall_progress_new() {
        let progress = OverallProgress::new(5, 5000);
        assert_eq!(progress.total_files, 5);
        assert_eq!(progress.completed_files, 0);
        assert_eq!(progress.pending_files, 5);
        assert_eq!(progress.total_bytes, 5000);
        assert_eq!(progress.downloaded_bytes, 0);
        assert_eq!(progress.percentage(), 0.0);
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_overall_progress_percentage() {
        let mut progress = OverallProgress::new(5, 5000);
        progress.downloaded_bytes = 2500;
        assert_eq!(progress.percentage(), 50.0);
    }

    #[test]
    fn test_overall_progress_complete() {
        let mut progress = OverallProgress::new(5, 5000);
        assert!(!progress.is_complete());

        progress.completed_files = 5;
        assert!(progress.is_complete());

        progress.completed_files = 3;
        progress.failed_files = 2;
        assert!(progress.is_complete());
    }

    #[test]
    fn test_progress_tracker_new() {
        let tracker = ProgressTracker::new(100);
        assert_eq!(tracker.start_bytes, 100);
        assert_eq!(tracker.last_bytes, 100);
    }

    #[test]
    fn test_progress_tracker_update() {
        let mut tracker = ProgressTracker::new(0);

        // Simulate download progress
        std::thread::sleep(std::time::Duration::from_millis(200));
        let (speed, eta) = tracker.update(1024, 10240);

        // Speed should be calculated
        assert!(speed > 0.0 || tracker.speed_samples.is_empty());

        // ETA should be estimated if speed > 0
        if speed > 0.0 {
            assert!(eta.is_some());
        }
    }

    #[test]
    fn test_progress_tracker_overall_stats() {
        let mut tracker = ProgressTracker::new(0);
        std::thread::sleep(std::time::Duration::from_millis(100));
        tracker.update(1024, 10240);

        let (_avg_speed, elapsed) = tracker.overall_stats();
        assert!(elapsed.as_millis() >= 100);
        // avg_speed might be 0 if update didn't trigger speed calculation
    }
}
