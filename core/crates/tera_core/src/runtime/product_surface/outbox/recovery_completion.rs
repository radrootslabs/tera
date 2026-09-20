use super::super::recovery_completion::{
    RecoveryCompletionReceipt, RecoveryMedia, RecoveryNativeReceipt, admit, validate_history,
};
use super::super::recovery_reconciliation::RecoveryDecision;
use super::*;

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn recover_legacy_upload(
        &self,
        native: RecoveryNativeReceipt,
        source: RecoveryMedia,
    ) -> Result<RecoveryCompletionReceipt, Phase1DraftError> {
        let admission = self.mutations.draft(*native.identity.parent.as_bytes())?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(native.identity.parent)
            .await
            .map_err(map_draft_storage_error)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.author() != &self.draft_author()? || head.scope().is_some() {
            return Err(Phase1DraftError::Corrupt);
        }
        let historical = storage
            .authored_draft_revision(native.identity.parent, native.identity.revision)
            .await
            .map_err(map_draft_storage_error)?
            .ok_or(Phase1DraftError::InvalidMedia)?;
        validate_history(&head, &historical, &native)?;
        let payload = Phase1DraftPayload::decode(&head)?;
        let old = Phase1DraftPayload::decode(&historical)?;
        let select = |payload: &Phase1DraftPayload| {
            let mut matches = payload
                .media
                .iter()
                .filter(|media| media.local_reference() == source.reference);
            let value = matches
                .next()
                .cloned()
                .ok_or(Phase1DraftError::InvalidMedia)?;
            if matches.next().is_some() {
                return Err(Phase1DraftError::InvalidMedia);
            }
            Ok(value)
        };
        let media = select(&payload)?;
        let prior = select(&old)?;
        let captured = old
            .form
            .as_ref()
            .and_then(|form| {
                form.media
                    .iter()
                    .find(|item| item.opaque_reference == source.reference)
            })
            .ok_or(Phase1DraftError::InvalidMedia)?;
        if radroots_sdk::transport::BlossomImageDimensions::new(captured.width, captured.height)
            .map_err(|_| Phase1DraftError::InvalidMedia)?
            != source.dimensions
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        let author =
            PublicKey::from_bytes(self.draft_author()?).map_err(|_| Phase1DraftError::Corrupt)?;
        if admit(author, &media, &prior, &native, &source)? == RecoveryDecision::Settle {
            return RecoveryCompletionReceipt::durable(&native, &media);
        }
        if head.stage().is_terminal() || head.operation_id().is_some() {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let now = phase1_operation_now_unix_ms()?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        let transaction = blossom
            .prepare_upload(source.request_at(now)?)
            .map_err(|_| Phase1DraftError::InvalidMedia)?;
        if transaction.expected_url().as_str() != media.url() {
            return Err(Phase1DraftError::InvalidMedia);
        }
        let (status, kind, encoding, body) = native.response.parts();
        let receipt = blossom
            .complete_native_upload(
                transaction,
                status,
                kind,
                encoding,
                body,
                radroots_sdk::transport::BlossomCancellation::default(),
            )
            .await
            .map_err(|_| Phase1DraftError::Operation)?;
        let mut verified = media.clone();
        verified.complete_recovered_transfer(&receipt)?;
        self.update_draft_media(
            &admission,
            *head.draft_id().as_bytes(),
            head.revision().get(),
            media.url(),
            now.max(head.updated_at_unix_ms()),
            |value| value.complete_recovered_transfer(&receipt),
        )
        .await?;
        RecoveryCompletionReceipt::durable(&native, &verified)
    }
}
