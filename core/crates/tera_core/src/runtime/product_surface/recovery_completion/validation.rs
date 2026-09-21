use radroots_blossom::{BlobDescriptor, BlobUrl, MediaType};
use radroots_identity::PublicKey;
use radroots_storage::authored_draft::AuthoredDraft;

use super::super::{
    Phase1DraftError as E, Phase1MediaPrerequisite, Phase1MediaStage, recovery_reconciliation::*,
};
use super::{RecoveryCompletionReceipt, RecoveryMedia, RecoveryNativeReceipt};

pub(in crate::runtime::product_surface) fn validate_history(
    head: &AuthoredDraft,
    historical: &AuthoredDraft,
    native: &RecoveryNativeReceipt,
) -> Result<(), E> {
    historical.validate().map_err(|_| E::Corrupt)?;
    if historical.draft_id() != native.identity.parent
        || historical.revision() != native.identity.revision
        || historical.draft_id() != head.draft_id()
        || historical.author() != head.author()
        || historical.scope() != head.scope()
        || historical.payload_schema() != head.payload_schema()
        || historical.created_at_unix_ms() != head.created_at_unix_ms()
        || historical.updated_at_unix_ms() > head.updated_at_unix_ms()
        || historical.revision() > head.revision()
        || (historical.revision() == head.revision() && historical != head)
    {
        return Err(E::Corrupt);
    }
    Ok(())
}

/// Local consistency admission only. Initial remote verification still requires
/// the shared SDK's canonical retrieval and its opaque completion receipt.
pub(in crate::runtime::product_surface) fn admit(
    author: PublicKey,
    current: &Phase1MediaPrerequisite,
    historical: &Phase1MediaPrerequisite,
    native: &RecoveryNativeReceipt,
    source: &RecoveryMedia,
) -> Result<RecoveryDecision, E> {
    if !current.same_input_as(historical)
        || !current.retains_authorization(historical)
        || historical.stage() != Phase1MediaStage::Uploading
        || current.local_reference() != source.reference
        || current.sha256() != native.media.hash.to_hex()
        || current.media_type() != native.media.media_type.as_str()
        || current.byte_size() != native.media.byte_size
        || source.media_type != native.media.media_type
    {
        return Err(E::InvalidMedia);
    }
    let url = BlobUrl::parse(current.url()).map_err(|_| E::InvalidMedia)?;
    if url.upload_url() != native.identity.upload_url
        && !(current.is_remote_verified() && url.as_str() == native.identity.upload_url)
    {
        return Err(E::InvalidMedia);
    }
    let (status, media_type, encoding, body) = native.response.parts();
    if !matches!(status, 200 | 201)
        || media_type != Some("application/json")
        || encoding.is_some_and(|value| value != "identity")
        || body.is_empty()
    {
        return Err(E::InvalidMedia);
    }
    let descriptor: BlobDescriptor = serde_json::from_slice(body).map_err(|_| E::InvalidMedia)?;
    let checked = descriptor
        .approve_reference()
        .and_then(|value| value.verify_bytes(&source.bytes, &source.media_type))
        .map_err(|_| E::InvalidMedia)?;
    let expected = RecoveryAssociation::new(
        author,
        native.identity.parent,
        historical.recovery_attempt()?,
        url,
        MediaType::parse(current.media_type()).map_err(|_| E::InvalidMedia)?,
        current.byte_size(),
    )?;
    let observed = RecoveryAssociation::new(
        author,
        native.identity.parent,
        native.identity.attempt,
        checked.descriptor().url().clone(),
        native.media.media_type.clone(),
        native.media.byte_size,
    )?;
    let decision = reconcile(
        &RecoveryParent::Known {
            association: Box::new(expected),
            completion: if current.is_remote_verified() {
                RustCompletion::Verified
            } else {
                RustCompletion::Pending
            },
        },
        &NativeRecoveryEvidence::Known {
            association: Box::new(observed),
            state: NativeRecoveryState::ReceiptAvailable,
        },
    );
    if !matches!(
        decision,
        RecoveryDecision::Complete | RecoveryDecision::Settle
    ) {
        return Err(E::InvalidMedia);
    }
    Ok(decision)
}

impl RecoveryCompletionReceipt {
    pub(in crate::runtime::product_surface) fn durable(
        native: &RecoveryNativeReceipt,
        media: &Phase1MediaPrerequisite,
    ) -> Result<Self, E> {
        if !media.is_remote_verified() || !media.retains_attempt(native.identity.attempt) {
            return Err(E::InvalidMedia);
        }
        let verified_at_unix_ms = media
            .verified_at_unix_ms()
            .filter(|value| (1..=i64::MAX as u64).contains(value))
            .ok_or(E::Corrupt)?;
        Ok(Self {
            parent: *native.identity.parent.as_bytes(),
            attempt: *native.identity.attempt.as_bytes(),
            canonical_url: media.url().into(),
            sha256: media.sha256().into(),
            media_type: media.media_type().into(),
            byte_size: media.byte_size(),
            verified_at_unix_ms,
        })
    }
}
