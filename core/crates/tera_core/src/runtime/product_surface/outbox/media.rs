//! Shared media transitions; only verified SDK receipts can satisfy a prerequisite.

use super::*;

impl Phase1MediaPrerequisite {
    pub(in crate::runtime::product_surface) fn transition_requested(
        &mut self,
        stage: Phase1MediaStage,
        failure_code: Option<String>,
    ) -> Result<(), Phase1DraftError> {
        if !valid_media_transition(self.stage, stage)
            || matches!(
                stage,
                Phase1MediaStage::Verified | Phase1MediaStage::Orphaned
            )
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        self.stage = stage;
        self.failure_code = failure_code;
        if stage != Phase1MediaStage::Failed {
            self.failure_code = None;
        }
        self.validate()?;
        Ok(())
    }

    pub(in crate::runtime::product_surface) fn complete_transfer(
        &mut self,
        receipt: &radroots_sdk::transport::BlossomUploadReceipt,
    ) -> Result<(), Phase1DraftError> {
        if self.stage != Phase1MediaStage::Uploading || !self.matches_receipt(receipt) {
            return Err(Phase1DraftError::InvalidMedia);
        }
        self.stage = Phase1MediaStage::Verified;
        self.failure_code = None;
        self.upload_attempts = receipt.attempts();
        self.verified_at_unix_ms = Some(receipt.verified_at_unix_ms());
        self.orphan = None;
        self.validate()?;
        Ok(())
    }

    pub(in crate::runtime::product_surface) fn fail_transfer(
        &mut self,
        error: &radroots_sdk::transport::BlossomError,
        updated_at_unix_ms: u64,
    ) -> Result<(), Phase1DraftError> {
        if !matches!(
            self.stage,
            Phase1MediaStage::Pending
                | Phase1MediaStage::Preparing
                | Phase1MediaStage::Uploading
                | Phase1MediaStage::Failed
        ) {
            return Err(Phase1DraftError::InvalidMedia);
        }
        self.stage = Phase1MediaStage::Failed;
        self.failure_code = Some(error.code().to_owned());
        self.upload_attempts = error.attempts();
        self.verified_at_unix_ms = None;
        self.orphan = error.possible_orphan().then(|| Phase1MediaOrphanRecord {
            reason_code: error.code().to_owned(),
            recorded_at_unix_ms: updated_at_unix_ms,
        });
        self.validate()?;
        Ok(())
    }

    pub(in crate::runtime::product_surface) fn same_input_as(&self, other: &Self) -> bool {
        self.local_reference == other.local_reference
            && self.url == other.url
            && self.sha256 == other.sha256
            && self.media_type == other.media_type
            && self.byte_size == other.byte_size
    }
}

impl TeraRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn update_draft_media(
        &self,
        _admission: &crate::runtime::mutation_admission::MutationPermit<'_>,
        draft_id: [u8; 16],
        expected_revision: u64,
        url: &str,
        updated_at_unix_ms: u64,
        update: impl FnOnce(&mut Phase1MediaPrerequisite) -> Result<(), Phase1DraftError>,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected
            || head.stage().is_terminal()
            || matches!(
                head.stage(),
                AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued
            )
        {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let mut payload = Phase1DraftPayload::decode(&head)?;
        let media = payload
            .media
            .iter_mut()
            .find(|media| media.url == url)
            .ok_or(Phase1DraftError::InvalidMedia)?;
        update(media)?;
        let next_stage = draft_stage_for_media(&payload.media);
        let next = head
            .successor(payload.encode()?, next_stage, None, updated_at_unix_ms)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let receipt = storage
            .append_authored_draft(next.clone(), Some(expected))
            .await
            .map_err(map_draft_storage_error)?;
        if receipt.draft() != &next {
            return Err(Phase1DraftError::Storage);
        }
        self.draft_status_from(receipt.draft().clone()).await
    }
}
