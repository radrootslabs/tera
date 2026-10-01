//! Prepare one explicitly selected independent retraction without delivery.
use super::*;

impl TeraRuntime {
    /// Success proves signed bytes are retained, not that any relay erased data.
    /// The host must retain this store and drain the runtime before key removal.
    pub async fn prepare_retraction_for_key_removal(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        let admission = self.mutations.draft(draft_id)?;
        let id = AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let head = self
            .storage()?
            .authored_draft_head(id)
            .await
            .map_err(Phase1DraftError::storage_error)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision().get() != expected_revision {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let status = self.draft_status_from(head.clone()).await?;
        if status.kind() != Phase1DraftKind::Retraction
            || status.revision_parent_draft_id().is_some()
        {
            return Err(Phase1DraftError::InvalidDraft);
        }
        let payload = Phase1DraftPayload::decode(&head)?;
        let plan = PlanWireV1::from_json(&payload.plan_wire_json)
            .map_err(|_| Phase1DraftError::Corrupt)?;
        self.require_publication_source(plan.plan(), Some(&head))
            .await?;
        // Never revive stopped work. Already-signed requests, including their
        // accepted/unknown outcomes, remain exact retained facts on replay.
        if status
            .push()
            .is_some_and(|push| push.artifact().signed().is_some())
        {
            return Ok(status);
        }
        let queued = self
            .phase1_queue_draft_admitted(
                &admission,
                draft_id,
                expected_revision,
                None,
                phase1_operation_now_unix_ms()?,
            )
            .await?;
        let signed = self
            .sign_queued_draft_admitted(draft_id, queued.draft().revision().get())
            .await?;
        if !signed
            .push()
            .is_some_and(|push| push.artifact().signed().is_some())
        {
            return Err(Phase1DraftError::OperationUnavailable);
        }
        Ok(signed)
    }
}
