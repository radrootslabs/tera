use crate::{
    FfiPreparedMediaInput, FfiRuntimeChangeKind, FfiSubmissionUploadResponse,
    MOBILE_FFI_SCHEMA_VERSION, TeraAppError, TeraRuntime,
    dto::{PreparedMedia, decode_id},
};
use tera_core::runtime::product_surface::{
    SubmissionMediaResponse,
    recovery_completion::{
        RecoveryCompletionError, RecoveryCompletionReceipt, RecoveryNativeIdentity,
        RecoveryNativeMedia, RecoveryNativeReceipt,
    },
};

#[derive(Clone, uniffi::Record)]
pub struct FfiRecoveryUploadReceipt {
    pub schema_version: u16,
    pub parent: String,
    pub revision: u64,
    pub attempt: String,
    pub upload_url: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub response: FfiSubmissionUploadResponse,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRecoveryCompletionReceipt {
    pub schema_version: u16,
    pub parent: String,
    pub attempt: String,
    pub canonical_url: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub verified_at_unix_ms: u64,
}

impl TryFrom<FfiRecoveryUploadReceipt> for RecoveryNativeReceipt {
    type Error = TeraAppError;
    fn try_from(value: FfiRecoveryUploadReceipt) -> Result<Self, Self::Error> {
        let invalid = || TeraAppError::invalid_argument("invalid_recovery_receipt");
        if value.schema_version != MOBILE_FFI_SCHEMA_VERSION
            || value.response.schema_version != MOBILE_FFI_SCHEMA_VERSION
            || value.sha256.len() != 64
            || value.media_type.len() > 8192
        {
            return Err(invalid());
        }
        let identity = RecoveryNativeIdentity::new(
            decode_id(&value.parent, "invalid_recovery_parent")?,
            value.revision,
            decode_id(&value.attempt, "invalid_recovery_attempt")?,
            value.upload_url,
        )?;
        let hash = radroots_blossom::Sha256::from_hex(&value.sha256).map_err(|_| invalid())?;
        let kind = radroots_blossom::MediaType::parse(&value.media_type).map_err(|_| invalid())?;
        let media = RecoveryNativeMedia::new(hash, kind, value.byte_size)?;
        let response = SubmissionMediaResponse::new(
            value.response.status_code,
            value.response.media_type,
            value.response.content_encoding,
            value.response.body,
        )?;
        Ok(Self::new(identity, media, response))
    }
}

impl From<RecoveryCompletionReceipt> for FfiRecoveryCompletionReceipt {
    fn from(value: RecoveryCompletionReceipt) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            parent: hex::encode(value.parent),
            attempt: hex::encode(value.attempt),
            canonical_url: value.canonical_url,
            sha256: value.sha256,
            media_type: value.media_type,
            byte_size: value.byte_size,
            verified_at_unix_ms: value.verified_at_unix_ms,
        }
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    pub async fn recover_native_upload(
        &self,
        receipt: FfiRecoveryUploadReceipt,
        media: FfiPreparedMediaInput,
    ) -> Result<FfiRecoveryCompletionReceipt, TeraAppError> {
        // Bound native evidence before opening or hashing any caller-owned file.
        let native = receipt.try_into()?;
        let source = PreparedMedia::try_from(media)?.into_recovery_media()?;
        let result = self.inner.recover_native_upload(native, source).await;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        result.map(Into::into).map_err(|error| match error {
            RecoveryCompletionError::Draft(error) => error.into(),
            RecoveryCompletionError::Submission(error) => error.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> FfiRecoveryUploadReceipt {
        FfiRecoveryUploadReceipt {
            schema_version: 1,
            parent: "01".repeat(16),
            revision: 2,
            attempt: "02".repeat(16),
            upload_url: "http://127.0.0.1:3000/upload".into(),
            sha256: "03".repeat(32),
            media_type: "image/png".into(),
            byte_size: 24,
            response: FfiSubmissionUploadResponse {
                schema_version: 1,
                status_code: 200,
                media_type: Some("application/json".into()),
                content_encoding: None,
                body: vec![1; 16_368],
            },
        }
    }

    #[test]
    fn recovery_native_evidence_bounds_reject_before_owned_media_admission() {
        // Exactly 16 KiB across the body and headers is admitted for subsequent
        // semantic validation; the next byte must fail at this boundary.
        assert!(RecoveryNativeReceipt::try_from(input()).is_ok());
        for case in 0..10 {
            let mut value = input();
            match case {
                0 => value.response.body.push(1),
                1 => value.schema_version = 2,
                2 => value.response.schema_version = 2,
                3 => value.parent = "00".repeat(16),
                4 => value.attempt = "00".repeat(16),
                5 => value.revision = 0,
                6 => value.revision = u64::MAX,
                7 => value.upload_url = "x".repeat(4097),
                8 => value.byte_size = 10 * 1024 * 1024 + 1,
                _ => value.media_type = "x".repeat(8193),
            }
            assert!(
                RecoveryNativeReceipt::try_from(value).is_err(),
                "case {case}"
            );
        }
    }
}
