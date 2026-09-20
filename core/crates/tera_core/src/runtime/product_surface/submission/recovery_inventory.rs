use radroots_storage::{Storage, authored_draft::AuthoredDraft};

use super::{
    SubmissionCommitError, SubmissionOperationError as E, SubmissionReservationRequest, intent,
    record, repository::SubmissionRepository,
};

/// Recover immutable identity from this exact intent, independently of UI scope.
/// This only reads existing state; it never reserves, prepares or advances work.
pub(in crate::runtime::product_surface) async fn recovery_request(
    stored: &AuthoredDraft,
    author: [u8; 32],
    store: &dyn Storage,
) -> Result<SubmissionReservationRequest, E> {
    let id = intent::inventory_reservation(stored, author)?;
    let reservation = store.authored_draft_head(id).await?.ok_or(E::Corrupt)?;
    let request =
        record::inventory_request(&reservation, author).map_err(SubmissionCommitError::from)?;
    let receipt = SubmissionRepository { store }
        .replay(&request)
        .await
        .map_err(SubmissionCommitError::from)?
        .ok_or(E::Corrupt)?;
    intent::validate_inventory(stored, &receipt)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::super::{operation_test_support::*, test_support::*};
    use crate::runtime::product_surface::{Phase1MediaStage, recovery_inventory::RecoveryOwner};

    #[tokio::test]
    async fn exact_pending_submission_parent_survives_unrelated_ui_pages_and_restart() {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let pending = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(pending.media()[0].stage(), Phase1MediaStage::Pending);
        let key = *pending.intent().draft_id().as_bytes();
        // Fill the unrelated display inventory after the pending operation.
        let seed = runtime
            .phase1_save_draft(
                1_u128.to_be_bytes(),
                crate::runtime::product_surface::Phase1AddCommand::CreateUpdate(
                    crate::runtime::product_surface::CreateUpdate::new("newer displayed work")
                        .unwrap(),
                ),
                NOW / 1000,
                vec![],
                None,
                NOW,
            )
            .await
            .unwrap()
            .draft()
            .clone();
        let store = runtime.client.storage().unwrap();
        for value in 2_u128..=1000 {
            let row = radroots_storage::authored_draft::AuthoredDraft::initial(
                radroots_storage::authored_draft::AuthoredDraftId::new(value.to_be_bytes())
                    .unwrap(),
                *seed.author(),
                seed.payload_schema(),
                seed.payload().to_vec(),
                seed.stage(),
                None,
                NOW,
            )
            .unwrap();
            store.append_authored_draft(row, None).await.unwrap();
        }
        assert_eq!(runtime.phase1_draft_heads(100).await.unwrap().len(), 100);
        assert_eq!(
            runtime.recovery_parent(key).await.unwrap().unwrap().owner,
            RecoveryOwner::Submission(request.clone())
        );
        runtime.shutdown().await.unwrap();
        let runtime =
            self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        assert_eq!(
            runtime.recovery_parent(key).await.unwrap().unwrap().owner,
            RecoveryOwner::Submission(request.clone())
        );
        let mut cursor = None;
        let mut found = false;
        loop {
            let page = runtime.recovery_page(37, cursor.as_deref()).await.unwrap();
            found |= page.entries.iter().any(|entry| {
                entry.key == key && entry.owner == RecoveryOwner::Submission(request.clone())
            });
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert!(found);
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
    }
}
