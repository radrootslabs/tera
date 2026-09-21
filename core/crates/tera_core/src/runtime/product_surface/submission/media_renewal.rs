//! An explicit host assertion accompanies, but never replaces, durable admission.

use super::*;
use crate::runtime::product_surface::{Phase1UploadPlan, phase1_operation_now_unix_ms};
use radroots_signing::SigningOperationId;
use radroots_storage::{Error, authored_draft::AuthoredDraftRevision};

/// The Apple host must hold native inactivity admission for the entire lineage
/// until this operation returns. Active, unknown and retained-response states
/// cannot construct this request. This assertion is transient, not stored proof.
#[derive(Clone, Debug)]
pub struct SubmissionUploadRenewal {
    prior_revision: AuthoredDraftRevision,
    prior_attempt: SigningOperationId,
    native_failed: bool,
}

impl SubmissionUploadRenewal {
    pub fn new(
        prior_revision: u64,
        prior_attempt: [u8; 16],
        native_failed: bool,
    ) -> Result<Self, E> {
        Ok(Self {
            prior_revision: AuthoredDraftRevision::new(prior_revision)
                .map_err(|_| E::InvalidMedia)?,
            prior_attempt: SigningOperationId::new(prior_attempt).map_err(|_| E::InvalidMedia)?,
            native_failed,
        })
    }
}

impl TeraRuntime {
    pub async fn submission_renew_native_upload(
        &self,
        input: SubmissionMediaRequest,
        renewal: SubmissionUploadRenewal,
    ) -> Result<
        (
            super::super::SubmissionOperationStatus,
            crate::runtime::product_surface::Phase1NativeUploadJob,
        ),
        E,
    > {
        self.prepare_submission_upload_at(input, Some(renewal), phase1_operation_now_unix_ms()?)
            .await
    }

    pub async fn submission_renew_upload_media(
        &self,
        input: SubmissionMediaRequest,
        renewal: SubmissionUploadRenewal,
    ) -> Result<super::super::SubmissionOperationStatus, E> {
        self.upload_submission_media_at(input, Some(renewal), phase1_operation_now_unix_ms()?)
            .await
    }

    pub(super) async fn reserve_submission_upload(
        &self,
        loaded: &mut LoadedOperation,
        input: &SubmissionMediaRequest,
        transaction: &BlossomUploadTransaction,
        plan: &Phase1UploadPlan,
        renewal: Option<SubmissionUploadRenewal>,
        now: u64,
    ) -> Result<(), E> {
        let revision = loaded
            .head
            .revision()
            .get()
            .checked_add(1)
            .ok_or(E::Corrupt)?;
        if let Some(renewal) = renewal {
            let store = self
                .client
                .storage()
                .map_err(|_| Error::BackendUnavailable)?;
            let historical = store
                .authored_draft_revision(loaded.head.draft_id(), renewal.prior_revision)
                .await?
                .ok_or(E::InvalidMedia)?;
            historical.validate().map_err(|_| E::Corrupt)?;
            if historical.draft_id() != loaded.head.draft_id()
                || historical.revision() != renewal.prior_revision
                || historical.author() != loaded.head.author()
                || historical.scope() != loaded.head.scope()
                || historical.payload_schema() != loaded.head.payload_schema()
                || historical.created_at_unix_ms() != loaded.head.created_at_unix_ms()
                || historical.revision() > loaded.head.revision()
                || historical.updated_at_unix_ms() > loaded.head.updated_at_unix_ms()
                || now < loaded.head.updated_at_unix_ms()
            {
                return Err(E::InvalidMedia);
            }
            let mut old = loaded
                .payload
                .current(&historical, loaded.receipt.operation_id())?;
            let prior = old.media_mut(&input.reference)?;
            let current = loaded.payload.media_mut(&input.reference)?;
            if prior.stage() != Phase1MediaStage::Uploading
                || prior.recovery_attempt()? != renewal.prior_attempt
                || current.recovery_attempt()? != renewal.prior_attempt
                || !current.retains_authorization(prior)
                || current
                    .upload_authorizations()
                    .last()
                    .and_then(|value| value.revision)
                    .is_some_and(|value| value != renewal.prior_revision.get())
            {
                return Err(E::InvalidMedia);
            }
            current.renew_upload(plan, transaction, revision, now, renewal.native_failed)?;
        } else {
            let media = loaded.payload.media_mut(&input.reference)?;
            media
                .reserve_upload(plan, transaction)
                .map_err(|_| E::InvalidMedia)?;
            media.associate_upload_revision(revision, now)?;
        }
        Ok(())
    }
}
