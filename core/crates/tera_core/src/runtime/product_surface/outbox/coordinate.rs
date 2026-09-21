//! Legacy capture adapter for the shared application coordinate authority.

use super::*;
use crate::runtime::product_surface::coordinate::CoordinatePlan;

pub(in crate::runtime::product_surface) fn coordinate_plan(
    draft: &AuthoredDraft,
) -> Result<Option<CoordinatePlan>, Phase1DraftError> {
    let payload = Phase1DraftPayload::decode(draft)?;
    let plan =
        PlanWireV1::from_json(&payload.plan_wire_json).map_err(|_| Phase1DraftError::Corrupt)?;
    let prior = payload
        .revision
        .as_ref()
        .filter(|revision| revision.policy == Phase1RevisionPolicy::AddressableReplacement)
        .map(|revision| {
            radroots_event::EventId::parse(&revision.target.source_event_id)
                .map(|id| *id.as_bytes())
                .map_err(|_| Phase1DraftError::Corrupt)
        })
        .transpose()?;
    CoordinatePlan::from_plan(draft, plan.plan(), prior)
}

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn legacy_coordinate_retirable(
        &self,
        head: &AuthoredDraft,
    ) -> Result<bool, Phase1DraftError> {
        coordinate_plan(head)?.ok_or(Phase1DraftError::Corrupt)?;
        if head.stage() == AuthoredDraftStage::Cancelled {
            return Ok(true);
        }
        Ok(self.push_status_for(head).await?.is_some_and(|push| {
            push.delivery_plan().stop_requested_at_unix_ms().is_some()
                || push.delivery_plan().state().is_terminal()
        }))
    }
}

impl TeraRuntime {
    /// Invokes the configured opaque host signer for one durably queued draft.
    ///
    /// The canonical sync engine verifies author, event ID, exact fields,
    /// signature, deadline, cancellation, and operation binding before the
    /// signed artifact can be persisted. Delivery remains a separate phase.
    pub async fn phase1_sign_queued_draft(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        let _admission = self.mutations.draft(draft_id)?;
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let head = self
            .storage()?
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected || head.stage() != AuthoredDraftStage::Queued {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let _coordinate = self.admit_coordinate(&head).await?;
        let _parent_admission = self.revision_parent_admission(&head)?;
        if matches!(
            self.revision_delivery_selection(&head).await?,
            RevisionDelivery::Held
        ) {
            return self.draft_status_from(head).await;
        }
        self.require_legacy_publication_running(sync_id_for(&head)?)
            .await?;
        let request = push_request(&head)?;
        self.require_publication_source(request.plan(), Some(&head))
            .await?;
        self.sync()?
            .sign_prepared(request)
            .await
            .map_err(|_| Phase1DraftError::Operation)?;
        self.require_draft_coordinate_current(&head).await?;
        self.draft_status_from(head).await
    }
}
