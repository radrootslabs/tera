//! Stable action identity and exact saved source; no publication acknowledgement.

use crate::{
    FfiComposerDraftRecord, FfiComposerScopeRecord, MOBILE_FFI_SCHEMA_VERSION, TeraAppError,
};
use tera_core::runtime::product_surface::{
    ComposerRevision, SubmissionCommandId, SubmissionReservationError,
    SubmissionReservationReceipt, SubmissionReservationRequest,
};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionCommandIdRecord {
    pub schema_version: u16,
    pub id: String,
}

/// Generate once for each intentional Submit and retain it for every retry.
#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn submission_reserve_id() -> Result<FfiSubmissionCommandIdRecord, TeraAppError> {
    Ok(FfiSubmissionCommandIdRecord {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        id: hex::encode(SubmissionCommandId::generate()?.as_bytes()),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionReservationRequest {
    pub schema_version: u16,
    pub command_id: String,
    pub scope: FfiComposerScopeRecord,
    pub composer_id: String,
    pub expected_revision: u64,
}

impl TryFrom<FfiSubmissionReservationRequest> for SubmissionReservationRequest {
    type Error = TeraAppError;
    fn try_from(value: FfiSubmissionReservationRequest) -> Result<Self, Self::Error> {
        if value.schema_version != MOBILE_FFI_SCHEMA_VERSION {
            return Err(SubmissionReservationError::UnsupportedSchema.into());
        }
        if value.command_id.len() != 32
            || !value
                .command_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SubmissionReservationError::InvalidCommandId.into());
        }
        let command_id = SubmissionCommandId::new(crate::decode_id(
            &value.command_id,
            "submission_command_id_invalid",
        )?)?;
        Ok(Self::new(
            command_id,
            value.scope.try_into()?,
            crate::composer::id(&value.composer_id)?,
            ComposerRevision::new(value.expected_revision)?,
        ))
    }
}

/// Confirms only the committed reservation and its immutable historical composer.
#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionReservationReceipt {
    pub schema_version: u16,
    pub command_id: String,
    pub reservation_id: String,
    pub captured: FfiComposerDraftRecord,
    pub reserved_at_unix_ms: u64,
    pub replayed: bool,
}

impl From<&SubmissionReservationReceipt> for FfiSubmissionReservationReceipt {
    fn from(value: &SubmissionReservationReceipt) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_id: hex::encode(value.request().command_id().as_bytes()),
            reservation_id: hex::encode(value.reservation_id().as_bytes()),
            captured: value.captured().into(),
            reserved_at_unix_ms: value.reserved_at_unix_ms(),
            replayed: value.is_replay(),
        }
    }
}

impl From<SubmissionReservationError> for TeraAppError {
    fn from(error: SubmissionReservationError) -> Self {
        use SubmissionReservationError as E;
        let (code, retryable, actions): (_, _, &[&str]) = match error {
            E::Source(error) => return error.into(),
            E::Lifecycle(error) => return tera_core::TeraAppError::from(error).into(),
            E::InvalidCommandId => return Self::invalid_argument("submission_command_id_invalid"),
            E::IdempotencyConflict => {
                ("idempotency_conflict", false, &["restore_original_request"])
            }
            E::CorruptRecord => (
                "submission_record_corrupt",
                false,
                &["preserve_local_work", "inspect_local_stores"],
            ),
            E::UnsupportedSchema => (
                "submission_schema_unsupported",
                false,
                &["preserve_local_work", "update_app"],
            ),
            E::InvalidReceipt => ("submission_receipt_mismatch", true, &["retry_same_command"]),
            E::ClockUnavailable => ("operation_clock_unavailable", true, &["check_device_clock"]),
            E::Storage(_) => (
                "submission_storage_failed",
                true,
                &["inspect_local_stores", "retry_same_command"],
            ),
        };
        Self::failure(
            code,
            "submission",
            retryable,
            actions,
            "The submission reservation could not be confirmed.",
        )
    }
}
