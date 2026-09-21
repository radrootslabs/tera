use crate::{FfiSubmissionUploadJobRecord, TeraAppError};
use tera_core::runtime::product_surface::{
    Phase1NativeUploadJob, SubmissionOperationStatus, SubmissionUploadRenewal,
};

/// Transient host reconciliation assertion, not reusable persisted authority.
#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionUploadRenewal {
    pub prior_revision: u64,
    pub prior_attempt: String,
    pub native_failed: bool,
}

impl TryFrom<FfiSubmissionUploadRenewal> for SubmissionUploadRenewal {
    type Error = TeraAppError;
    fn try_from(value: FfiSubmissionUploadRenewal) -> Result<Self, Self::Error> {
        let bytes = crate::decode_id(&value.prior_attempt, "submission_upload_attempt_invalid")?;
        if hex::encode(bytes) != value.prior_attempt {
            return Err(TeraAppError::invalid_argument(
                "submission_upload_attempt_invalid",
            ));
        }
        Ok(Self::new(value.prior_revision, bytes, value.native_failed)?)
    }
}

impl From<(SubmissionOperationStatus, Phase1NativeUploadJob)> for FfiSubmissionUploadJobRecord {
    fn from((status, job): (SubmissionOperationStatus, Phase1NativeUploadJob)) -> Self {
        Self {
            schema_version: crate::UPLOAD_OUTPUT_FFI_SCHEMA_VERSION,
            submission: (&status).into(),
            operation_id: hex::encode(job.operation_id()),
            remote_url: job.remote_url().to_owned(),
            upload_url: job.upload_url().to_owned(),
            authorization_header: job.authorization_header().to_owned(),
            expected_sha256: job.expected_sha256().to_owned(),
            media_type: job.media_type().to_owned(),
            byte_size: job.byte_size(),
        }
    }
}
