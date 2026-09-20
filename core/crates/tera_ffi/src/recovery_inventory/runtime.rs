use super::TeraRuntime;
use crate::{
    FfiRecoveryEntry, FfiRecoveryPage, MOBILE_FFI_SCHEMA_VERSION, TeraAppError, decode_id,
};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    pub async fn recovery_page(
        &self,
        schema_version: u16,
        limit: u16,
        cursor: Option<String>,
    ) -> Result<FfiRecoveryPage, TeraAppError> {
        require_version(schema_version)?;
        let page = self.inner.recovery_page(limit, cursor.as_deref()).await?;
        Ok((&page).into())
    }

    pub async fn recovery_parent(
        &self,
        schema_version: u16,
        key: String,
    ) -> Result<Option<FfiRecoveryEntry>, TeraAppError> {
        require_version(schema_version)?;
        Ok(self
            .inner
            .recovery_parent(decode_id(&key, "invalid_recovery_key")?)
            .await?
            .as_ref()
            .map(Into::into))
    }
}

fn require_version(version: u16) -> Result<(), TeraAppError> {
    if version != MOBILE_FFI_SCHEMA_VERSION {
        return Err(TeraAppError::invalid_argument("unsupported_schema_version"));
    }
    Ok(())
}
