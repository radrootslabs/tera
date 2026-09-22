//! Guarded construction loads saved adapters without rewriting original work.
use radroots_sdk::transport::BlossomConfig;

use crate::{TeraAppError, TeraRuntime};

impl TeraRuntime {
    pub(crate) async fn install_restored_preferences(&self) -> Result<(), TeraAppError> {
        if self.restore_guard.is_none() {
            return Err(TeraAppError::runtime("restore_recovery_required"));
        }
        let settings = self
            .phase1_settings()
            .await
            .map_err(|error| TeraAppError::runtime(error.code()))?;
        let relays = settings
            .relays()
            .sdk_profile()
            .map_err(|error| TeraAppError::runtime(error.code()))?;
        let blossom = settings
            .blossom()
            .sdk_profile()
            .map_err(|error| TeraAppError::runtime(error.code()))?;
        // The ordinary configuration path deliberately narrows/stops work.
        // Startup recovery must only install inert adapters; the durable hold
        // and later explicit review/resume retain publication authority.
        self.client
            .configure_nostr(relays)
            .map_err(TeraAppError::from_sdk)?;
        self.client
            .configure_blossom(BlossomConfig::from_profile(blossom))
            .map_err(TeraAppError::from_sdk)
    }
}
