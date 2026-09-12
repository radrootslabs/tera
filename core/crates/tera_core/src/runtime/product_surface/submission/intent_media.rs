//! Mutable prerequisite evidence cannot replace any field of the captured request.

use super::*;
use crate::runtime::product_surface::submission::SubmissionOperationError;

type OperationError = SubmissionOperationError;

impl IntentPayload {
    pub(in crate::runtime::product_surface::submission) fn current(
        &self,
        head: &AuthoredDraft,
        operation: OperationInstanceId,
    ) -> Result<Self, OperationError> {
        let current: Self =
            serde_json::from_slice(head.payload()).map_err(|_| OperationError::Corrupt)?;
        let mut immutable = current.clone();
        immutable.media.clone_from(&self.media);
        if immutable != *self || current.media.len() != self.media.len() {
            return Err(OperationError::Corrupt);
        }
        for (original, media) in self.media.iter().zip(&current.media) {
            media.validate().map_err(|_| OperationError::Corrupt)?;
            if !original.same_input_as(media) || media.stage() == Phase1MediaStage::Orphaned {
                return Err(OperationError::Corrupt);
            }
        }
        let stage = current.stage();
        let valid_stage = head.stage() == stage
            || (stage == AuthoredDraftStage::ReadyToSign
                && head.stage() == AuthoredDraftStage::Queued);
        let association = (stage == AuthoredDraftStage::ReadyToSign).then_some(operation);
        if !valid_stage || head.operation_id() != association {
            return Err(OperationError::Corrupt);
        }
        Ok(current)
    }

    pub(in crate::runtime::product_surface::submission) fn media(
        &self,
    ) -> &[Phase1MediaPrerequisite] {
        &self.media
    }

    pub(in crate::runtime::product_surface::submission) fn media_mut(
        &mut self,
        reference: &str,
    ) -> Result<&mut Phase1MediaPrerequisite, OperationError> {
        let mut matches = self
            .media
            .iter_mut()
            .filter(|media| media.local_reference() == reference);
        let media = matches.next().ok_or(OperationError::InvalidMedia)?;
        if matches.next().is_some() {
            return Err(OperationError::InvalidMedia);
        }
        Ok(media)
    }

    pub(in crate::runtime::product_surface::submission) fn media_policy(&self) -> Option<[u8; 32]> {
        self.media_policy
    }

    pub(in crate::runtime::product_surface::submission) fn encode(
        &self,
    ) -> Result<Vec<u8>, OperationError> {
        let bytes = serde_json::to_vec(self).map_err(|_| OperationError::Corrupt)?;
        if bytes.len() > SUBMISSION_INTENT_MAX_BYTES {
            return Err(OperationError::Corrupt);
        }
        Ok(bytes)
    }

    pub(in crate::runtime::product_surface::submission) fn stage(&self) -> AuthoredDraftStage {
        if self
            .media
            .iter()
            .all(Phase1MediaPrerequisite::is_remote_verified)
        {
            AuthoredDraftStage::ReadyToSign
        } else if self
            .media
            .iter()
            .any(|media| media.stage() == Phase1MediaStage::Uploading)
        {
            AuthoredDraftStage::MediaUploading
        } else {
            AuthoredDraftStage::MediaPreparing
        }
    }
}
