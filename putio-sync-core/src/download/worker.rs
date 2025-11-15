//! Worker pool for concurrent downloads

use crate::error::{CoreError, CoreResult};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex, Semaphore};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// A download job to be processed by a worker
#[derive(Debug, Clone)]
pub struct DownloadJob {
    /// Unique identifier for this job
    pub id: String,
    /// File ID from Put.io
    pub file_id: i64,
    /// Filename
    pub filename: String,
    /// File size in bytes
    pub file_size: i64,
    /// Priority (higher = more important)
    pub priority: i32,
}

/// Result of a download operation
#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// Job that was processed
    pub job: DownloadJob,
    /// Whether the download succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Number of bytes downloaded
    pub bytes_downloaded: i64,
    /// Duration in seconds
    pub duration_seconds: i64,
}

/// Worker pool for managing concurrent downloads
pub struct WorkerPool {
    /// Number of workers
    worker_count: usize,
    /// Semaphore for limiting concurrent downloads
    semaphore: Arc<Semaphore>,
    /// Channel for sending jobs to workers (shared among workers)
    #[allow(dead_code)]
    job_rx: Arc<Mutex<mpsc::UnboundedReceiver<DownloadJob>>>,
    /// Channel for sending jobs
    job_tx: mpsc::UnboundedSender<DownloadJob>,
    /// Channel for receiving results from workers
    result_rx: mpsc::UnboundedReceiver<DownloadResult>,
    /// Worker task handles
    worker_handles: Vec<JoinHandle<()>>,
    /// Shutdown signal
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl WorkerPool {
    /// Create a new worker pool
    ///
    /// # Arguments
    ///
    /// * `worker_count` - Number of concurrent workers
    ///
    /// # Examples
    ///
    /// ```
    /// use putio_sync_core::download::WorkerPool;
    ///
    /// let pool = WorkerPool::new(2);
    /// ```
    pub fn new(worker_count: usize) -> Self {
        let worker_count = worker_count.max(1); // At least 1 worker
        let semaphore = Arc::new(Semaphore::new(worker_count));
        let (job_tx, job_rx) = mpsc::unbounded_channel::<DownloadJob>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, _) = broadcast::channel(1);

        info!("Creating worker pool with {} workers", worker_count);

        let mut worker_handles = Vec::new();

        // Create workers
        for worker_id in 0..worker_count {
            let semaphore = semaphore.clone();
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();

            let handle = tokio::spawn(async move {
                debug!("Worker {} started", worker_id);

                loop {
                    tokio::select! {
                        // Check for shutdown signal
                        _ = shutdown_rx.recv() => {
                            debug!("Worker {} received shutdown signal", worker_id);
                            break;
                        }
                        // Try to get a job from the shared receiver
                        job = async {
                            let mut rx = job_rx.lock().await;
                            rx.recv().await
                        } => {
                            match job {
                                Some(job) => {
                                    debug!("Worker {} received job: {}", worker_id, job.id);

                                    // Acquire semaphore permit
                                    let permit = semaphore.acquire().await;
                                    if permit.is_err() {
                                        error!("Worker {} failed to acquire semaphore", worker_id);
                                        continue;
                                    }
                                    let _permit = permit.unwrap();

                                    // Process the job (placeholder for now)
                                    let result = Self::process_job(worker_id, job).await;

                                    // Send result
                                    if let Err(e) = result_tx.send(result) {
                                        error!("Worker {} failed to send result: {}", worker_id, e);
                                    }

                                    // Permit is automatically released when dropped
                                }
                                None => {
                                    debug!("Worker {} channel closed", worker_id);
                                    break;
                                }
                            }
                        }
                    }
                }

                debug!("Worker {} stopped", worker_id);
            });

            worker_handles.push(handle);
        }

        // Drop the extra result_tx so the channel closes when all workers are done
        drop(result_tx);

        Self {
            worker_count,
            semaphore,
            job_rx,
            job_tx,
            result_rx,
            worker_handles,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Submit a job to the worker pool
    ///
    /// # Arguments
    ///
    /// * `job` - The download job to process
    pub fn submit_job(&self, job: DownloadJob) -> CoreResult<()> {
        debug!("Submitting job: {}", job.id);

        self.job_tx
            .send(job)
            .map_err(|e| CoreError::InternalError(format!("Failed to submit job: {}", e)))?;

        Ok(())
    }

    /// Get the next result from the worker pool
    ///
    /// Returns None if there are no more results
    pub async fn next_result(&mut self) -> Option<DownloadResult> {
        self.result_rx.recv().await
    }

    /// Shutdown the worker pool gracefully
    ///
    /// This will wait for all workers to finish their current jobs
    pub async fn shutdown(mut self) -> CoreResult<()> {
        info!("Shutting down worker pool");

        // Send shutdown signal to all workers
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            drop(shutdown_tx); // Dropping sends signal to all subscribers
        }

        // Wait for all workers to finish
        for (i, handle) in self.worker_handles.into_iter().enumerate() {
            match handle.await {
                Ok(_) => debug!("Worker {} finished successfully", i),
                Err(e) => warn!("Worker {} finished with error: {}", i, e),
            }
        }

        info!("Worker pool shutdown complete");
        Ok(())
    }

    /// Get the number of workers in the pool
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    /// Get the number of available worker slots
    pub fn available_workers(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Process a download job (placeholder implementation)
    async fn process_job(worker_id: usize, job: DownloadJob) -> DownloadResult {
        debug!("Worker {} processing job: {}", worker_id, job.id);

        // Simulate download work
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Placeholder result
        DownloadResult {
            job: job.clone(),
            success: true,
            error: None,
            bytes_downloaded: job.file_size,
            duration_seconds: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_pool_creation() {
        let pool = WorkerPool::new(2);
        assert_eq!(pool.worker_count(), 2);
        assert_eq!(pool.available_workers(), 2);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_worker_pool_single_worker() {
        let pool = WorkerPool::new(1);
        assert_eq!(pool.worker_count(), 1);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_worker_pool_zero_workers() {
        // Should default to 1 worker
        let pool = WorkerPool::new(0);
        assert_eq!(pool.worker_count(), 1);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_submit_job() {
        let pool = WorkerPool::new(2);

        let job = DownloadJob {
            id: "test-1".to_string(),
            file_id: 12345,
            filename: "test.txt".to_string(),
            file_size: 1024,
            priority: 1,
        };

        pool.submit_job(job).unwrap();

        // Give workers time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_process_job() {
        let mut pool = WorkerPool::new(2);

        let job = DownloadJob {
            id: "test-1".to_string(),
            file_id: 12345,
            filename: "test.txt".to_string(),
            file_size: 1024,
            priority: 1,
        };

        pool.submit_job(job.clone()).unwrap();

        // Wait for result
        let result = pool.next_result().await;
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.job.id, "test-1");
        assert!(result.success);
        assert_eq!(result.bytes_downloaded, 1024);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_jobs() {
        let mut pool = WorkerPool::new(2);

        // Submit multiple jobs
        for i in 0..5 {
            let job = DownloadJob {
                id: format!("test-{}", i),
                file_id: i,
                filename: format!("test-{}.txt", i),
                file_size: 1024 * (i + 1),
                priority: i as i32,
            };

            pool.submit_job(job).unwrap();
        }

        // Collect results
        let mut results = Vec::new();
        while let Some(result) = pool.next_result().await {
            results.push(result);
            if results.len() == 5 {
                break;
            }
        }

        assert_eq!(results.len(), 5);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_limit() {
        let pool = WorkerPool::new(2);

        // Submit 3 jobs (more than worker count)
        for i in 0..3 {
            let job = DownloadJob {
                id: format!("test-{}", i),
                file_id: i,
                filename: format!("test-{}.txt", i),
                file_size: 1024,
                priority: 1,
            };

            pool.submit_job(job).unwrap();
        }

        // Only 2 should be running at a time
        // After submitting, available workers should be less than total
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(pool.available_workers() < pool.worker_count());

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let pool = WorkerPool::new(2);

        // Submit a job
        let job = DownloadJob {
            id: "test-1".to_string(),
            file_id: 12345,
            filename: "test.txt".to_string(),
            file_size: 1024,
            priority: 1,
        };

        pool.submit_job(job).unwrap();

        // Wait a bit for job to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Shutdown should wait for job to complete
        pool.shutdown().await.unwrap();
    }
}
