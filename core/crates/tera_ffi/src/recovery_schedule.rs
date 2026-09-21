use crate::{TeraAppError, TeraRuntime};
use tera_core::runtime::product_surface::recovery_schedule::NativeRecoverySchedule;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiNativeRecoverySchedule {
    pub schema_version: u16,
    pub author: String,
    pub revision: u64,
    pub after: Option<String>,
}

impl From<NativeRecoverySchedule> for FfiNativeRecoverySchedule {
    fn from(value: NativeRecoverySchedule) -> Self {
        Self {
            schema_version: value.schema_version,
            author: hex::encode(value.author),
            revision: value.revision,
            after: value.after.map(hex::encode),
        }
    }
}

fn digest(value: &str) -> Result<[u8; 32], TeraAppError> {
    let invalid = || TeraAppError::invalid_argument("invalid_recovery_schedule");
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut bytes = [0; 32];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| invalid())?;
    Ok(bytes)
}

impl TryFrom<FfiNativeRecoverySchedule> for NativeRecoverySchedule {
    type Error = TeraAppError;
    fn try_from(value: FfiNativeRecoverySchedule) -> Result<Self, Self::Error> {
        if value.schema_version != 1
            || value.revision > i64::MAX as u64
            || (value.revision == 0 && value.after.is_some())
        {
            return Err(TeraAppError::invalid_argument("invalid_recovery_schedule"));
        }
        Ok(Self {
            schema_version: 1,
            author: digest(&value.author)?,
            revision: value.revision,
            after: value.after.as_deref().map(digest).transpose()?,
        })
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    pub async fn native_recovery_schedule(
        &self,
        schema_version: u16,
    ) -> Result<FfiNativeRecoverySchedule, TeraAppError> {
        if schema_version != 1 {
            return Err(TeraAppError::invalid_argument("invalid_recovery_schedule"));
        }
        self.inner
            .native_recovery_schedule()
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn advance_native_recovery_schedule(
        &self,
        expected: FfiNativeRecoverySchedule,
        after: Option<String>,
    ) -> Result<FfiNativeRecoverySchedule, TeraAppError> {
        self.inner
            .advance_native_recovery_schedule(
                expected.try_into()?,
                after.as_deref().map(digest).transpose()?,
            )
            .await
            .map(Into::into)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_versioned_bounded_canonical_cursors() {
        let initial = FfiNativeRecoverySchedule {
            schema_version: 1,
            author: "ab".repeat(32),
            revision: 0,
            after: None,
        };
        assert!(NativeRecoverySchedule::try_from(initial.clone()).is_ok());
        for value in ["", &"A".repeat(64), &"0".repeat(65), &"é".repeat(32)] {
            assert!(digest(value).is_err());
        }
        for value in [
            FfiNativeRecoverySchedule {
                schema_version: 2,
                ..initial.clone()
            },
            FfiNativeRecoverySchedule {
                revision: u64::MAX,
                ..initial.clone()
            },
            FfiNativeRecoverySchedule {
                after: Some("00".repeat(32)),
                ..initial
            },
        ] {
            assert!(NativeRecoverySchedule::try_from(value).is_err());
        }
    }
}
