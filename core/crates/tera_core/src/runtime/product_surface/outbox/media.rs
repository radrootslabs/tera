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
