//! Validate the immutable transaction before using any mutable operation head.

use radroots_storage::{
    authored_atomic::{AuthoredAtomicOutcome, AuthoredAtomicStorage, PrepareAuthoredOperation},
    authored_draft::{AuthoredDraft, AuthoredDraftStore},
};
use radroots_sync::{PushRequest, PushStatus};

use super::{
    SubmissionReceipt, SubmissionReservationRequest,
    intent::{self, IntentPayload},
    operation::SubmissionOperationError as E,
    repository::SubmissionRepository,
};

pub(super) struct LoadedOperation {
    pub receipt: SubmissionReceipt,
    pub head: AuthoredDraft,
    pub request: PushRequest,
    pub captured: crate::runtime::product_surface::ComposerDraft,
    pub payload: IntentPayload,
    preparation: PrepareAuthoredOperation,
}

impl<S: AuthoredDraftStore + AuthoredAtomicStorage + ?Sized> SubmissionRepository<'_, S> {
    pub(super) async fn load_operation(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<LoadedOperation, E> {
        let atomic = self
            .store
            .authored_receipt(intent::commit_id(request))
            .await?
            .ok_or(E::NotFound)?;
        let receipt = self
            .committed_receipt(request, atomic.clone(), true)
            .await?;
        let AuthoredAtomicOutcome::Submitted(original) = atomic.outcome() else {
            return Err(E::Corrupt);
        };
        let reservation = self
            .replay(request)
            .await
            .map_err(super::SubmissionCommitError::from)?
            .ok_or(E::Corrupt)?;
        let payload: IntentPayload =
            serde_json::from_slice(original.intent().payload()).map_err(|_| E::Corrupt)?;
        let push = payload.push_request(&reservation)?;
        let head = self
            .store
            .authored_draft_head(receipt.intent_id())
            .await?
            .ok_or(E::Corrupt)?;
        validate_head(original.intent(), &head)?;
        let current = payload.current(&head, receipt.operation_id())?;
        Ok(LoadedOperation {
            receipt,
            head,
            request: push,
            captured: reservation.captured().clone(),
            payload: current,
            preparation: original.preparation().clone(),
        })
    }
}

fn validate_head(original: &AuthoredDraft, head: &AuthoredDraft) -> Result<(), E> {
    head.validate().map_err(|_| E::Corrupt)?;
    if head.draft_id() != original.draft_id()
        || head.author() != original.author()
        || head.scope() != original.scope()
        || head.payload_schema() != original.payload_schema()
        || head.created_at_unix_ms() != original.created_at_unix_ms()
        || head.updated_at_unix_ms() < original.updated_at_unix_ms()
        || head.updated_at_unix_ms() > i64::MAX as u64
        || head.revision() < original.revision()
        || (head.revision() == original.revision() && head != original)
    {
        return Err(E::Corrupt);
    }
    Ok(())
}

impl LoadedOperation {
    pub(super) fn validate_push(&self, push: &PushStatus) -> Result<(), E> {
        let [artifact] = self.preparation.artifacts() else {
            return Err(E::Corrupt);
        };
        let [delivery] = self.preparation.delivery_plans() else {
            return Err(E::Corrupt);
        };
        if push.operation().operation_id() != self.receipt.operation_id()
            || push.operation().artifact_ids() != self.preparation.operation().artifact_ids()
            || push.operation().created_at_unix_ms()
                != self.preparation.operation().created_at_unix_ms()
            || push.artifact().artifact_id() != artifact.artifact_id()
            || push.artifact().operation_id() != artifact.operation_id()
            || push.artifact().ordinal() != artifact.ordinal()
            || push.artifact().origin() != artifact.origin()
            || push.artifact().plan() != artifact.plan()
            || push.artifact().created_at_unix_ms() != artifact.created_at_unix_ms()
            || push.delivery_plan().plan_id() != delivery.plan_id()
            || push.delivery_plan().artifact_id() != delivery.artifact_id()
            || push.delivery_plan().intent() != delivery.intent()
            || push.delivery_plan().created_at_unix_ms() != delivery.created_at_unix_ms()
        {
            return Err(E::Corrupt);
        }
        Ok(())
    }
}
