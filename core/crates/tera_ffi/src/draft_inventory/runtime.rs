use super::TeraRuntime;
use crate::{FfiLegacyDraftPageRecord, MOBILE_FFI_SCHEMA_VERSION, TeraAppError};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    /// Returns bounded legacy selection metadata and individual repair entries.
    /// Legacy ownership is author-bound and unscoped; no composer context is inferred.
    pub async fn legacy_draft_page(
        &self,
        schema_version: u16,
        limit: u16,
        cursor: Option<String>,
    ) -> Result<FfiLegacyDraftPageRecord, TeraAppError> {
        if schema_version != MOBILE_FFI_SCHEMA_VERSION {
            return Err(TeraAppError::invalid_argument("unsupported_schema_version"));
        }
        let page = self
            .inner
            .phase1_draft_page(limit, cursor.as_deref())
            .await
            .map_err(TeraAppError::from)?;
        Ok((&page).into())
    }
}
