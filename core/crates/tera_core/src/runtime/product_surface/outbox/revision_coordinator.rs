//! Ordered revision coordination over durable replacement and child facts.

use super::*;

impl TeraRuntime {
    /// Advances replacement and deletion independently per frozen target.
    /// A deletion target requires its own replacement acceptance/read-back.
    pub async fn phase1_advance_revision(
        &self,
        replacement_draft_id: [u8; 16],
    ) -> Result<Phase1RevisionStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        let admission = self.mutations.draft(replacement_draft_id)?;
        let mut status = self.phase1_revision_status(replacement_draft_id).await?;
        if revision_status::stopped(&status.replacement) {
            return Ok(status);
        }
        if matches!(
            status.replacement.state(),
            Phase1OutboxState::Draft | Phase1OutboxState::ReadyToSign
        ) && status.replacement_progress.can_resume
        {
            let now_unix_ms = phase1_operation_now_unix_ms()?;
            self.phase1_queue_draft_admitted(
                &admission,
                replacement_draft_id,
                status.replacement.draft().revision().get(),
                None,
                now_unix_ms,
            )
            .await?;
            status = self.phase1_revision_status(replacement_draft_id).await?;
        }
        if status.replacement_progress.can_resume
            && status.replacement.draft().stage() == AuthoredDraftStage::Queued
        {
            self.phase1_advance_draft_admitted(
                &admission,
                replacement_draft_id,
                status.replacement.draft().revision().get(),
            )
            .await?;
            status = self.phase1_revision_status(replacement_draft_id).await?;
        }
        if status.policy != Phase1RevisionPolicy::ReplaceThenRetract
            || !self
                .revision_has_replacement_evidence(&status.replacement)
                .await?
        {
            return Ok(status);
        }

        let child_id =
            revision_child_id(status.replacement.draft())?.ok_or(Phase1DraftError::Corrupt)?;
        if status.retraction.is_none() {
            self.phase1_create_revision_retraction(&status.target, child_id, replacement_draft_id)
                .await?;
            status = self.phase1_revision_status(replacement_draft_id).await?;
        }
        let child = status
            .retraction
            .as_ref()
            .ok_or(Phase1DraftError::Corrupt)?;
        if revision_status::stopped(child)
            || child.revision_parent_draft_id() != Some(replacement_draft_id)
        {
            return Ok(status);
        }
        if matches!(
            child.state(),
            Phase1OutboxState::Draft | Phase1OutboxState::ReadyToSign
        ) {
            self.phase1_queue_add_intent(Phase1QueueIntent::new(
                child_id,
                child.draft().revision().get(),
            )?)
            .await?;
            status = self.phase1_revision_status(replacement_draft_id).await?;
        }
        let child = status
            .retraction
            .as_ref()
            .ok_or(Phase1DraftError::Corrupt)?;
        if status
            .retraction_progress
            .as_ref()
            .is_some_and(|branch| branch.can_resume)
            && child.draft().stage() == AuthoredDraftStage::Queued
        {
            drop(admission);
            self.phase1_advance_draft(child_id, child.draft().revision().get())
                .await?;
        }
        self.phase1_revision_status(replacement_draft_id).await
    }

    /// Cancels only still-pending work. If a kind-1 replacement is already
    /// visible, a cancelled child records the deliberate partial effect and
    /// prevents a later recovery from retracting the original unexpectedly.
    pub async fn phase1_cancel_revision(
        &self,
        replacement_draft_id: [u8; 16],
    ) -> Result<Phase1RevisionStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        let admission = self.mutations.draft(replacement_draft_id)?;
        let mut status = self.phase1_revision_status(replacement_draft_id).await?;
        if !revision_status::stopped(&status.replacement)
            && !matches!(
                status.replacement.state(),
                Phase1OutboxState::Complete
                    | Phase1OutboxState::Terminal
                    | Phase1OutboxState::Cancelled
            )
        {
            self.phase1_cancel_draft_admitted(
                &admission,
                replacement_draft_id,
                status.replacement.draft().revision().get(),
                phase1_operation_now_unix_ms()?,
            )
            .await?;
            status = self.phase1_revision_status(replacement_draft_id).await?;
        }
        if (status.replacement.state() == Phase1OutboxState::Complete
            || status.retraction.is_some())
            && status.policy == Phase1RevisionPolicy::ReplaceThenRetract
        {
            let child_id =
                revision_child_id(status.replacement.draft())?.ok_or(Phase1DraftError::Corrupt)?;
            if status.retraction.is_none() {
                self.phase1_create_revision_retraction(
                    &status.target,
                    child_id,
                    replacement_draft_id,
                )
                .await?;
                status = self.phase1_revision_status(replacement_draft_id).await?;
            }
            let child = status
                .retraction
                .as_ref()
                .ok_or(Phase1DraftError::Corrupt)?;
            if !revision_status::stopped(child)
                && !matches!(
                    child.state(),
                    Phase1OutboxState::Complete
                        | Phase1OutboxState::Terminal
                        | Phase1OutboxState::Cancelled
                )
            {
                self.phase1_cancel_add_intent(child_id, child.draft().revision().get())
                    .await?;
            }
        }
        self.phase1_revision_status(replacement_draft_id).await
    }

    pub(super) async fn phase1_create_revision_retraction(
        &self,
        target: &Phase1RevisionTarget,
        draft_id: [u8; 16],
        replacement_draft_id: [u8; 16],
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let admission = self.mutations.draft(draft_id)?;
        match self.phase1_draft_status(draft_id).await {
            Ok(existing) => return Ok(existing),
            Err(Phase1DraftError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let now_unix_ms = phase1_operation_now_unix_ms()?;
        self.phase1_save_retraction_draft_admitted(
            &admission,
            draft_id,
            target.command_type,
            target.card_id,
            &target.source_event_id,
            target.source_kind,
            target.source_address.as_deref(),
            REVISION_RETRACTION_REASON,
            now_unix_ms / 1_000,
            now_unix_ms,
            Some(replacement_draft_id),
        )
        .await
    }
}
