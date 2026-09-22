//! Existing queued schemas retain their original policy when configuration changes.

use super::*;
use radroots_sdk::transport::RelayProfile;

impl TeraRuntime {
    /// Called under exclusive publication configuration admission. Preparing a
    /// recovered ready row is local only and binds the original frozen request.
    /// No draft mutation slot or external signer/socket is awaited here.
    pub(crate) async fn restrict_legacy_publications(
        &self,
        relays: Option<&RelayProfile>,
        stop_all: bool,
    ) -> Result<(), Phase1DraftError> {
        let author = self.draft_author()?;
        let storage = self.storage()?;
        for schema in [DRAFT_PAYLOAD_SCHEMA, PROFILE_PAYLOAD_SCHEMA] {
            let mut query = AuthoredDraftQuery::for_author(author, schema, 32)
                .map_err(|_| Phase1DraftError::InvalidDraft)?;
            loop {
                let page = storage
                    .query_authored_drafts(query.clone())
                    .await
                    .map_err(map_draft_storage_error)?;
                let next = page.next_cursor().cloned();
                for row in page.into_records() {
                    let AuthoredDraftQueryRecord::Draft(draft) = row else {
                        continue;
                    };
                    if !matches!(
                        draft.stage(),
                        AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued
                    ) {
                        continue;
                    }
                    // These pure validators also gate every effect entrypoint.
                    // A malformed row remains repair data and cannot block a
                    // different healthy request. Backend errors still propagate.
                    let request = if schema == DRAFT_PAYLOAD_SCHEMA {
                        push_request(&draft)
                    } else {
                        profile_push_request(&draft)
                    };
                    let Ok(request) = request else {
                        continue;
                    };
                    if operation_id(draft.draft_id(), draft.payload()).ok()
                        != Some(request.operation_id())
                    {
                        continue;
                    }
                    let policy = if schema == DRAFT_PAYLOAD_SCHEMA {
                        Phase1DraftPayload::decode(&draft)?.queue
                    } else {
                        Phase1ProfilePayload::decode(&draft)?.queue
                    }
                    .ok_or(Phase1DraftError::Corrupt)?;
                    let removed = relays.is_some_and(|profile| {
                        policy.relay_urls.iter().any(|url| {
                            !profile.endpoints().iter().any(|endpoint| {
                                endpoint.access().can_write() && endpoint.url().as_str() == url
                            })
                        })
                    });
                    if stop_all || removed {
                        let operation = request.operation_id();
                        self.sync()?
                            .prepare_push(request)
                            .await
                            .map_err(|_| Phase1DraftError::Operation)?;
                        self.stop_legacy_publication(operation).await?;
                    }
                }
                let Some(next) = next else {
                    break;
                };
                query = query
                    .with_cursor(&next)
                    .map_err(|_| Phase1DraftError::Corrupt)?;
            }
        }
        Ok(())
    }

    async fn stop_legacy_publication(&self, operation: SyncId) -> Result<(), Phase1DraftError> {
        let sync = self.sync()?;
        let result = sync.cancel_push(operation).await;
        let stopped = sync
            .push_status(operation)
            .await
            .map_err(|_| Phase1DraftError::Operation)?
            .ok_or(Phase1DraftError::Corrupt)?;
        if stopped
            .delivery_plan()
            .stop_requested_at_unix_ms()
            .is_some()
        {
            return Ok(());
        }
        result.map_err(|_| Phase1DraftError::Operation)?;
        Err(Phase1DraftError::Corrupt)
    }

    pub(in crate::runtime::product_surface) async fn require_legacy_publication_running(
        &self,
        operation: SyncId,
    ) -> Result<(), Phase1DraftError> {
        self.require_restore_effects_allowed().await?;
        let configuration = self.publication_configuration.read().await;
        if !configuration.allowed {
            self.stop_legacy_publication(operation).await?;
            return Err(Phase1DraftError::Terminal);
        }
        let status = self
            .sync()?
            .push_status(operation)
            .await
            .map_err(|_| Phase1DraftError::Operation)?
            .ok_or(Phase1DraftError::Corrupt)?;
        if status.delivery_plan().stop_requested_at_unix_ms().is_some() {
            return Err(Phase1DraftError::Terminal);
        }
        Ok(())
    }
}
