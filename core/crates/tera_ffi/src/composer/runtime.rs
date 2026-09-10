use super::TeraRuntime;
use crate::composer::{id, version};
use crate::{
    FfiComposerDraftRecord, FfiComposerPageRecord, FfiComposerSaveReceipt, FfiComposerSaveRequest,
    FfiComposerScopeRecord, FfiRuntimeChangeKind, TeraAppError,
};
use tera_core::runtime::product_surface::{ComposerPartialForm, ComposerScope};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    /// Saves bounded editing data without validating publication or creating an operation.
    pub async fn composer_save(
        &self,
        request: FfiComposerSaveRequest,
    ) -> Result<FfiComposerSaveReceipt, TeraAppError> {
        let (id, expected_revision, sequence) = request.validate_identity()?;
        let scope: ComposerScope = request.scope.try_into()?;
        let form: ComposerPartialForm = request.form.try_into()?;
        let receipt = match expected_revision {
            Some(revision) => {
                self.inner
                    .composer_save(&scope, id, revision, sequence, form)
                    .await
            }
            None => self.inner.composer_create(&scope, id, sequence, form).await,
        }
        .map_err(TeraAppError::from)?;
        self.subscriptions.notify(
            FfiRuntimeChangeKind::Drafts,
            Some(hex::encode(id.as_bytes())),
        );
        Ok((&receipt).into())
    }

    pub async fn composer_load(
        &self,
        scope: FfiComposerScopeRecord,
        composer_id: String,
    ) -> Result<FfiComposerDraftRecord, TeraAppError> {
        let scope = scope.try_into()?;
        let draft = self
            .inner
            .composer_load(&scope, id(&composer_id)?)
            .await
            .map_err(TeraAppError::from)?;
        Ok((&draft).into())
    }

    /// Live ID-ordered inventory. Resnapshot from None to discover insertions behind a cursor.
    pub async fn composer_list(
        &self,
        schema_version: u16,
        scope: FfiComposerScopeRecord,
        limit: u16,
        cursor: Option<String>,
    ) -> Result<FfiComposerPageRecord, TeraAppError> {
        version(schema_version)?;
        let scope = scope.try_into()?;
        let page = self
            .inner
            .composer_list(&scope, limit, cursor.as_deref())
            .await
            .map_err(TeraAppError::from)?;
        Ok((&page).into())
    }
}
