//! Persist restrictions before acknowledging an endpoint or author change.

use super::*;
use radroots_sdk::transport::BlossomConfig;

impl TeraRuntime {
    /// Settings revision admission is held by the caller. The publication gate
    /// protects only local records and inert configuration, never network I/O.
    pub(super) async fn replace_settings_fenced(
        &self,
        command: ReplaceMobileSettings,
    ) -> Result<SettingsTransition, SettingsError> {
        let storage = self.client.storage().map_err(|_| SettingsError::Storage)?;
        let prior = load_settings(storage).await?;
        if prior.revision != command.expected_revision {
            return Err(SettingsError::RevisionConflict);
        }
        prior
            .revision
            .checked_add(1)
            .ok_or(SettingsError::RevisionExhausted)?;
        let next = &command.settings;
        let relays = (prior.relays != next.relays)
            .then(|| next.relays.sdk_profile())
            .transpose()?;
        let media = (prior.blossom != next.blossom)
            .then(|| next.blossom.sdk_profile().map(BlossomConfig::from_profile))
            .transpose()?;
        let author_changed = active_author(&prior.identity) != active_author(&next.identity);
        let mut configuration = self.publication_configuration.write().await;
        if relays.is_some() || media.is_some() || author_changed {
            configuration.invalidate_capture();
            self.restrict_publications(
                relays.as_ref(),
                media.as_ref().map(BlossomConfig::fingerprint),
                author_changed,
            )
            .await
            .map_err(|_| SettingsError::Storage)?;
        }
        let transition = replace_settings(storage, command).await?;
        // If applying an inert adapter fails after persistence, old policy must
        // not authorize new captures. The existing runtime restart is recovery.
        let allowed = if author_changed {
            active_author(&transition.settings.identity)
                == self.authenticated_store_public_key_hex().as_deref()
        } else {
            configuration.allowed
        };
        configuration.allowed = false;
        if let Some(relays) = relays {
            self.client
                .configure_nostr(relays)
                .map_err(|_| SettingsError::Storage)?;
        }
        if let Some(media) = media {
            self.client
                .configure_blossom(media)
                .map_err(|_| SettingsError::Storage)?;
        }
        configuration.allowed = allowed;
        Ok(transition)
    }
}

fn active_author(identity: &IdentityState) -> Option<&str> {
    let id = identity.active_identity_id()?;
    identity
        .identities()
        .iter()
        .find(|record| record.id() == id)
        .map(IdentityRecord::public_key_hex)
}
