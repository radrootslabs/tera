use super::super::{
    ComposerStorageRecord, composer,
    media_gc::{InventoryBudget, MediaInventoryIncomplete as E},
};
use super::{SubmissionReservationReceipt, intent, record};
use radroots_storage::{Storage, authored_draft::AuthoredDraft};

pub(in crate::runtime::product_surface) async fn media_references(
    stored: AuthoredDraft,
    author: [u8; 32],
    store: &dyn Storage,
    budget: &mut InventoryBudget,
) -> Result<Vec<String>, E> {
    let (reservation, intent) =
        if stored.payload_schema() == record::SUBMISSION_RESERVATION_PAYLOAD_SCHEMA {
            (stored, None)
        } else {
            let id = intent::inventory_reservation(&stored, author).map_err(|_| E)?;
            let reservation = store
                .authored_draft_head(id)
                .await
                .map_err(|_| E)?
                .ok_or(E)?;
            budget.record(&reservation)?;
            (reservation, Some(stored))
        };
    let request = record::inventory_request(&reservation, author).map_err(|_| E)?;
    let wire = record::decode(&reservation, &request).map_err(|_| E)?;
    let source = store
        .authored_draft_revision(wire.source.draft_id(), wire.source.revision())
        .await
        .map_err(|_| E)?
        .ok_or(E)?;
    budget.record(&source)?;
    if !wire.source.matches(&source)
        || source.updated_at_unix_ms() > reservation.created_at_unix_ms()
    {
        return Err(E);
    }
    let references = composer::media_references(source.clone(), author)?;
    let captured = ComposerStorageRecord::decode(source, request.scope())
        .map_err(|_| E)?
        .draft()
        .clone();
    let receipt = SubmissionReservationReceipt {
        request,
        reservation_id: reservation.draft_id(),
        captured,
        source: wire.source,
        reserved_at_unix_ms: reservation.created_at_unix_ms(),
        replayed: true,
    };
    if let Some(intent) = intent {
        intent::validate_inventory(&intent, &receipt).map_err(|_| E)?;
    }
    Ok(references)
}
