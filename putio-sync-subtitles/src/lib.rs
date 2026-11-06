//! # putio-sync-subtitles
//!
//! Subtitles for downloaded videos (feature 025): pick the best Put.io subtitle per wanted
//! language, convert it to UTF-8 SRT, save it next to the video without overwriting anything,
//! and, when none exists and transcription is on, make one with a Whisper-style server.
//!
//! The pipeline crate schedules; this crate does the work for one video at a time.

use std::future::Future;
use std::pin::Pin;

use putio_sync_client::{ClientError, PutioApiClient, PutioSubtitle, SubtitleList};

pub mod candidates;
pub mod naming;
pub mod srt;

/// Language codes (the table lives in `putio-sync-core`, where the config check needs it too)
pub use putio_sync_core::lang;

/// A boxed future, so [`SubtitleSource`] can be a trait object without a macro crate
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a Put.io call gave nothing
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// Put.io cannot give subtitles for this file (gone, no scope, plan): not a failure, the
    /// step moves on to transcription
    #[error("putio unavailable: {0}")]
    Unavailable(String),
    /// A real failure (network, 5xx, bad response): the step fails and is retried
    #[error("{0}")]
    Failed(String),
}

/// Put.io's subtitles for a file. A trait so the engine is tested without HTTP.
pub trait SubtitleSource: Send + Sync {
    /// What Put.io offers for `file_id` in `languages`
    fn list<'a>(
        &'a self,
        file_id: i64,
        languages: &'a [String],
    ) -> BoxFut<'a, Result<SubtitleList, SourceError>>;

    /// One subtitle's raw bytes
    fn download<'a>(
        &'a self,
        file_id: i64,
        subtitle: &'a PutioSubtitle,
    ) -> BoxFut<'a, Result<Vec<u8>, SourceError>>;
}

fn map_error(e: ClientError) -> SourceError {
    if e.is_subtitles_unavailable() {
        SourceError::Unavailable(e.to_string())
    } else {
        SourceError::Failed(e.to_string())
    }
}

impl SubtitleSource for PutioApiClient {
    fn list<'a>(
        &'a self,
        file_id: i64,
        languages: &'a [String],
    ) -> BoxFut<'a, Result<SubtitleList, SourceError>> {
        Box::pin(async move {
            self.list_subtitles(file_id, languages)
                .await
                .map_err(map_error)
        })
    }

    fn download<'a>(
        &'a self,
        file_id: i64,
        subtitle: &'a PutioSubtitle,
    ) -> BoxFut<'a, Result<Vec<u8>, SourceError>> {
        Box::pin(async move {
            self.download_subtitle(file_id, &subtitle.key, Some(&subtitle.url))
                .await
                .map_err(map_error)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn put_io_client_maps_404_to_unavailable_and_5xx_to_failed() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/files/1/subtitles")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/files/2/subtitles")
            .with_status(400)
            .with_body("{}")
            .create_async()
            .await;
        let c = PutioApiClient::with_base_url("t".into(), server.url());
        let source: &dyn SubtitleSource = &c;
        assert!(matches!(
            source.list(1, &[]).await,
            Err(SourceError::Unavailable(_))
        ));
        assert!(matches!(
            source.list(2, &[]).await,
            Err(SourceError::Failed(_))
        ));
    }
}
