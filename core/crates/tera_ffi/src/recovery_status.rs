use crate::{MOBILE_FFI_SCHEMA_VERSION, TeraAppError, TeraRuntime};
use tera_core::runtime::product_surface::recovery_status::{
    NativeRecoveryReason, NativeRecoveryStatus,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiNativeRecoveryReason {
    MissingParent,
    InvalidParent,
    AssociationMismatch,
    OutcomeUnconfirmed,
    Resolved,
}

impl From<FfiNativeRecoveryReason> for NativeRecoveryReason {
    fn from(value: FfiNativeRecoveryReason) -> Self {
        match value {
            FfiNativeRecoveryReason::MissingParent => Self::MissingParent,
            FfiNativeRecoveryReason::InvalidParent => Self::InvalidParent,
            FfiNativeRecoveryReason::AssociationMismatch => Self::AssociationMismatch,
            FfiNativeRecoveryReason::OutcomeUnconfirmed => Self::OutcomeUnconfirmed,
            FfiNativeRecoveryReason::Resolved => Self::Resolved,
        }
    }
}

impl From<NativeRecoveryReason> for FfiNativeRecoveryReason {
    fn from(value: NativeRecoveryReason) -> Self {
        match value {
            NativeRecoveryReason::MissingParent => Self::MissingParent,
            NativeRecoveryReason::InvalidParent => Self::InvalidParent,
            NativeRecoveryReason::AssociationMismatch => Self::AssociationMismatch,
            NativeRecoveryReason::OutcomeUnconfirmed => Self::OutcomeUnconfirmed,
            NativeRecoveryReason::Resolved => Self::Resolved,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiNativeRecoveryStatus {
    pub schema_version: u16,
    pub key: String,
    pub reason: FfiNativeRecoveryReason,
    pub revision: u64,
    pub first_observed_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl From<NativeRecoveryStatus> for FfiNativeRecoveryStatus {
    fn from(value: NativeRecoveryStatus) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            key: hex::encode(value.key),
            reason: value.reason.into(),
            revision: value.revision,
            first_observed_unix_ms: value.first_observed_unix_ms,
            updated_at_unix_ms: value.updated_at_unix_ms,
        }
    }
}

fn key(schema_version: u16, value: &str) -> Result<[u8; 32], TeraAppError> {
    if schema_version != MOBILE_FFI_SCHEMA_VERSION
        || value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(TeraAppError::invalid_argument(
            "invalid_native_recovery_status",
        ));
    }
    let mut result = [0; 32];
    hex::decode_to_slice(value, &mut result)
        .map_err(|_| TeraAppError::invalid_argument("invalid_native_recovery_status"))?;
    Ok(result)
}

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    pub async fn native_recovery_status(
        &self,
        schema_version: u16,
        transfer_key: String,
    ) -> Result<Option<FfiNativeRecoveryStatus>, TeraAppError> {
        self.inner
            .native_recovery_status(key(schema_version, &transfer_key)?)
            .await
            .map(|value| value.map(Into::into))
            .map_err(Into::into)
    }

    pub async fn report_native_recovery_status(
        &self,
        schema_version: u16,
        transfer_key: String,
        reason: FfiNativeRecoveryReason,
    ) -> Result<Option<FfiNativeRecoveryStatus>, TeraAppError> {
        self.inner
            .report_native_recovery_status(key(schema_version, &transfer_key)?, reason.into())
            .await
            .map(|value| value.map(Into::into))
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_key_admission_is_versioned_canonical_and_bounded() {
        assert_eq!(key(1, &"ab".repeat(32)).unwrap(), [0xab; 32]);
        for value in [
            "",
            &"A".repeat(64),
            &"0".repeat(63),
            &"0".repeat(65),
            &"é".repeat(32),
        ] {
            assert!(key(1, value).is_err());
        }
        assert!(key(2, &"0".repeat(64)).is_err());
        for reason in [
            FfiNativeRecoveryReason::MissingParent,
            FfiNativeRecoveryReason::InvalidParent,
            FfiNativeRecoveryReason::AssociationMismatch,
            FfiNativeRecoveryReason::OutcomeUnconfirmed,
            FfiNativeRecoveryReason::Resolved,
        ] {
            assert_eq!(
                FfiNativeRecoveryReason::from(NativeRecoveryReason::from(reason)),
                reason
            );
        }
    }
}
