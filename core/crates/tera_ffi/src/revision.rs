use crate::{FfiAddCommandType, FfiDraftFormRecord, TeraAppError, TeraRuntime};
use tera_core::runtime::product_surface::{CardId, Phase1RevisionTarget};

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiRevisionSourceRequest {
    pub schema_version: u16,
    pub source_draft_id: String,
    pub command_type: FfiAddCommandType,
    pub card_id: String,
    pub source_event_id: String,
    pub source_address: Option<String>,
    pub author_public_key: String,
}

#[uniffi::export]
impl TeraRuntime {
    pub async fn revision_source_form(
        &self,
        request: FfiRevisionSourceRequest,
    ) -> Result<FfiDraftFormRecord, TeraAppError> {
        if request.schema_version != crate::MOBILE_FFI_SCHEMA_VERSION {
            return Err(TeraAppError::invalid_argument(
                "unsupported_revision_source_schema",
            ));
        }
        let target = Phase1RevisionTarget::from_source(
            request.command_type.into(),
            CardId::parse(&request.card_id)
                .map_err(|_| TeraAppError::invalid_argument("invalid_card_id"))?,
            request.source_event_id,
            request.source_address,
            request.author_public_key,
        )?;
        let source = crate::decode_id(&request.source_draft_id, "invalid_revision_source_id")?;
        let form = self.inner.revision_source_form(source, &target).await?;
        Ok((&form).into())
    }
}
