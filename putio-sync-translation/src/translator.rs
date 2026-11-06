//! The translator the pipeline holds: the live `[translation]` settings and the limit on
//! chat requests in flight. Settings can change while the daemon runs.

use std::sync::{Arc, RwLock};

use putio_sync_core::translation::{TranslationConfig, TranslatorKind};
use tokio::sync::Semaphore;

use crate::chat::ChatClient;

/// Live settings plus the shared in-flight limit
pub struct Translator {
    config: RwLock<TranslationConfig>,
    permits: RwLock<Arc<Semaphore>>,
}

impl Translator {
    /// A translator for `config`
    pub fn new(config: &TranslationConfig) -> Translator {
        Translator {
            config: RwLock::new(config.clone()),
            permits: RwLock::new(Arc::new(Semaphore::new(config.max_in_flight.max(1)))),
        }
    }

    /// Use new settings from now on. Requests already running keep the limit they started
    /// under.
    pub fn configure(&self, config: &TranslationConfig) {
        let mut current = self.config.write().unwrap();
        if current.max_in_flight != config.max_in_flight {
            *self.permits.write().unwrap() = Arc::new(Semaphore::new(config.max_in_flight.max(1)));
        }
        *current = config.clone();
    }

    /// A copy of the current settings
    pub fn config(&self) -> TranslationConfig {
        self.config.read().unwrap().clone()
    }

    /// Which translator is selected
    pub fn kind(&self) -> TranslatorKind {
        self.config.read().unwrap().kind
    }

    /// A chat client for the current settings
    pub fn chat_client(&self) -> Result<ChatClient, String> {
        let c = self.config();
        ChatClient::new(
            &c.chat.base_url,
            &c.chat.model,
            &c.chat.effective_api_key(),
            c.timeout_secs,
            self.permits.read().unwrap().clone(),
        )
    }
}
