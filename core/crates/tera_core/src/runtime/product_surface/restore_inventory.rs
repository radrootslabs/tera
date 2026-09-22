//! Complete bounded restore inventory, including profiles and revision children.
use radroots_storage::authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord};
use radroots_sync::PushRequest;
use sha2::{Digest, Sha256};

use super::media_gc::{
    MEDIA_PAGE_BUDGET, MEDIA_PAGE_LIMIT, MEDIA_PAYLOAD_BYTE_BUDGET, MEDIA_RECORD_BUDGET,
};

pub(crate) const RESTORE_TARGET_BUDGET: usize = 4096;
use crate::{
    TeraRuntime,
    runtime::restore::{BARRIER_SCHEMA, RestoreError as E},
};

pub(crate) struct RestoreInventory {
    pub digest: [u8; 32],
    pub publications: Vec<([u8; 16], PushRequest)>,
}

impl TeraRuntime {
    /// Caller holds runtime maintenance across this traversal and its use.
    pub(crate) async fn restore_inventory(&self) -> Result<RestoreInventory, E> {
        let author = self
            .store_public_key
            .ok_or(E::IdentityMismatch)?
            .into_bytes();
        // The caller's maintenance lease excludes new commands. Settle both
        // owner pools as well: canceled SQLx work may outlive its command lease.
        self.client
            .storage_operations()
            .map_err(|_| E::Unavailable)?
            .settle_backup_writes()
            .await
            .map_err(|_| E::Unavailable)?;
        let store = self.client.storage().map_err(|_| E::Unavailable)?;
        // Reuse the complete application schema and historical-link validation.
        super::media_gc::backup_references(store, author)
            .await
            .map_err(|_| E::VerificationFailed)?;
        let mut query = AuthoredDraftQuery::for_author_all_schemas(author, MEDIA_PAGE_LIMIT)
            .map_err(|_| E::InvalidRequest)?;
        let mut digest = Sha256::new();
        digest.update(b"tera.restore_inventory.v1\0");
        digest.update(author);
        let mut publications = Vec::new();
        let mut target_count = 0_usize;
        let (mut records, mut bytes) = (0, 0);
        for _ in 0..MEDIA_PAGE_BUDGET {
            let page = store
                .query_authored_drafts(query.clone())
                .await
                .map_err(|_| E::Unavailable)?;
            let next = page.next_cursor().cloned();
            for row in page.into_records() {
                let AuthoredDraftQueryRecord::Draft(head) = row else {
                    return Err(E::VerificationFailed);
                };
                records += 1;
                bytes += head.payload().len();
                if records > MEDIA_RECORD_BUDGET || bytes > MEDIA_PAYLOAD_BYTE_BUDGET {
                    return Err(E::CapacityExceeded);
                }
                if matches!(
                    head.payload_schema(),
                    BARRIER_SCHEMA | crate::runtime::restore::REVIEW_SCHEMA
                ) {
                    continue;
                }
                digest.update(head.draft_id().as_bytes());
                digest.update(head.revision().get().to_be_bytes());
                digest.update(head.payload_sha256());
                let request = match head.payload_schema() {
                    "radroots.mobile.phase1-draft.v1" | "radroots.mobile.phase1-profile.v1" => self
                        .restore_legacy_request(&head)
                        .await
                        .map_err(|_| E::VerificationFailed)?,
                    super::SUBMISSION_INTENT_PAYLOAD_SCHEMA => self
                        .restore_submission_request(&head)
                        .await
                        .map_err(|_| E::VerificationFailed)?,
                    _ => None,
                };
                if let Some(request) = request {
                    target_count = target_count
                        .checked_add(request.targets().len())
                        .ok_or(E::CapacityExceeded)?;
                    if target_count > RESTORE_TARGET_BUDGET {
                        return Err(E::CapacityExceeded);
                    }
                    publications.push((*head.draft_id().as_bytes(), request));
                }
            }
            let Some(next) = next else {
                return Ok(RestoreInventory {
                    digest: digest.finalize().into(),
                    publications,
                });
            };
            query = query
                .with_cursor(&next)
                .map_err(|_| E::VerificationFailed)?;
        }
        Err(E::CapacityExceeded)
    }
}
