//! Independent and revision-child retraction capture; shared NIP09 construction.

use super::*;

impl TeraRuntime {
    /// Persists an independent strict NIP-09 retraction as a normal durable outbox item.
    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_save_retraction_draft(
        &self,
        draft_id: [u8; 16],
        command_type: AddCommandType,
        target_card_id: CardId,
        target_event_id: &str,
        target_kind: u32,
        target_address: Option<&str>,
        reason: &str,
        authored_at_unix_s: u64,
        persisted_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        let admission = self.mutations.draft(draft_id)?;
        let target = Phase1RevisionTarget::new(
            command_type,
            target_card_id,
            target_event_id,
            target_kind,
            target_address.map(str::to_owned),
            hex::encode(self.draft_author()?),
        )?;
        self.require_revision_source(&target).await?;
        self.phase1_save_retraction_draft_admitted(
            &admission,
            draft_id,
            command_type,
            target_card_id,
            target_event_id,
            target_kind,
            target_address,
            reason,
            authored_at_unix_s,
            persisted_at_unix_ms,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn phase1_save_retraction_draft_admitted(
        &self,
        _admission: &crate::runtime::mutation_admission::MutationPermit<'_>,
        draft_id: [u8; 16],
        command_type: AddCommandType,
        target_card_id: CardId,
        target_event_id: &str,
        target_kind: u32,
        target_address: Option<&str>,
        reason: &str,
        authored_at_unix_s: u64,
        persisted_at_unix_ms: u64,
        revision_parent_draft_id: Option<[u8; 16]>,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let target_shape_valid = match command_type {
            AddCommandType::CreateUpdate
            | AddCommandType::CreatePhotoUpdate
            | AddCommandType::CreateAsk => target_kind == 1 && target_address.is_none(),
            AddCommandType::CreateEvent => {
                matches!(target_kind, 31_922 | 31_923) && target_address.is_some()
            }
            AddCommandType::CreateFoodAvailability => {
                target_kind == 30_402 && target_address.is_some()
            }
        };
        if !target_shape_valid || authored_at_unix_s == 0 || persisted_at_unix_ms == 0 {
            return Err(Phase1DraftError::InvalidDraft);
        }
        let author = self.draft_author()?;
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let event_target = Nip09DeletionEventTarget::parse(target_event_id, target_kind)
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let address_targets = target_address
            .map(Nip09DeletionAddressTarget::parse)
            .transpose()
            .map_err(|_| Phase1DraftError::InvalidDraft)?
            .into_iter()
            .collect();
        let request =
            AuthoredNip09DeletionRequest::new(reason, vec![event_target], address_targets)
                .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let plan = phase1_retraction_plan(&request, authored_at_unix_s, hex::encode(author))
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let wire = PlanWireV1::from_plan(&plan)
            .to_json()
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let mut payload = Phase1DraftPayload::retraction(command_type, target_card_id, wire)?;
        payload.revision_parent_draft_id = revision_parent_draft_id;
        let bytes = payload.encode()?;
        let draft = AuthoredDraft::initial(
            draft_id,
            author,
            DRAFT_PAYLOAD_SCHEMA,
            bytes,
            AuthoredDraftStage::Draft,
            None,
            persisted_at_unix_ms,
        )
        .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let receipt = self
            .storage()?
            .append_authored_draft(draft, None)
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }
}
