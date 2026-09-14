//! Configuration may narrow existing intent; it never amends its destinations.

use radroots_sdk::transport::{BlossomConfigFingerprint, RelayProfile};
use radroots_storage::authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord};

use super::{
    SubmissionCommitError, SubmissionOperationError as E, SubmissionReservationError,
    intent::IntentPayload, operation_load::LoadedOperation,
};
use crate::{TeraRuntime, runtime::product_surface::Phase1DraftError};

/// Process-local capture admission, never part of a durable command identity.
pub(crate) struct PublicationConfiguration {
    pub token: [u8; 16],
    pub allowed: bool,
}

impl Default for PublicationConfiguration {
    fn default() -> Self {
        Self {
            token: *uuid::Uuid::new_v4().as_bytes(),
            allowed: true,
        }
    }
}

impl PublicationConfiguration {
    pub fn invalidate_capture(&mut self) {
        self.token = *uuid::Uuid::new_v4().as_bytes();
    }
}

impl TeraRuntime {
    /// Caller holds the exclusive configuration guard. Pages are bounded and
    /// stable-ID ordered; local intent installation cannot add missed rows.
    /// Cancellation or failure leaves already confirmed stops intact and does
    /// not acknowledge the configuration. No signer/socket is awaited here.
    /// Heap-own this bounded scan so its nested storage futures do not enlarge
    /// every settings/native caller's async state. It remains caller-owned;
    /// this creates no task or worker and retains ordinary drop cancellation.
    pub(crate) fn restrict_publications<'a>(
        &'a self,
        relays: Option<&'a RelayProfile>,
        media: Option<BlossomConfigFingerprint>,
        stop_all: bool,
    ) -> impl std::future::Future<Output = Result<(), E>> + Send + 'a {
        Box::pin(async move {
            let Some(author) = self.store_public_key else {
                return Ok(());
            };
            let store = self
                .client
                .storage()
                .map_err(|_| Phase1DraftError::Storage)?;
            let mut query = AuthoredDraftQuery::for_author(
                author.into_bytes(),
                super::SUBMISSION_INTENT_PAYLOAD_SCHEMA,
                32,
            )?;
            loop {
                let page = store.query_authored_drafts(query.clone()).await?;
                let next = page.next_cursor().cloned();
                for record in page.into_records() {
                    let AuthoredDraftQueryRecord::Draft(draft) = record else {
                        continue;
                    };
                    let Some(loaded) = self.configuration_operation(&draft).await? else {
                        continue;
                    };
                    if loaded.head.draft_id() != draft.draft_id() {
                        continue;
                    }
                    if stop_all || loaded.restricted_by(relays, media)? {
                        self.submission_request_stop(loaded.receipt.request())
                            .await?;
                    }
                }
                let Some(next) = next else {
                    break;
                };
                query = query.with_cursor(&next)?;
            }
            self.restrict_legacy_publications(relays, stop_all).await?;
            Ok(())
        })
    }

    async fn configuration_operation(
        &self,
        draft: &radroots_storage::authored_draft::AuthoredDraft,
    ) -> Result<Option<LoadedOperation>, E> {
        let result = async {
            let payload: IntentPayload =
                serde_json::from_slice(draft.payload()).map_err(|_| E::Corrupt)?;
            let store = self
                .client
                .storage()
                .map_err(|_| Phase1DraftError::Storage)?;
            let request = payload.reservation_request(store).await?;
            self.load_submission_operation(&request)
                .await
                .map(|(loaded, _)| loaded)
        }
        .await;
        match result {
            Ok(loaded) => Ok(Some(loaded)),
            // These same validating loaders reject effect admission. Leave the
            // row visible as repair in inventory; unrelated Add stays usable.
            Err(E::NotFound | E::Corrupt | E::Submission(
                SubmissionCommitError::CorruptRecord | SubmissionCommitError::UnsupportedSchema
                | SubmissionCommitError::InvalidIntent | SubmissionCommitError::InvalidReceipt
                | SubmissionCommitError::Storage(radroots_storage::Error::CorruptAuthoredDraft)
                | SubmissionCommitError::Reservation(SubmissionReservationError::CorruptRecord
                    | SubmissionReservationError::UnsupportedSchema | SubmissionReservationError::IdempotencyConflict
                    | SubmissionReservationError::Storage(radroots_storage::Error::CorruptAuthoredDraft)
                    | SubmissionReservationError::Source(crate::runtime::product_surface::ComposerPersistenceError::ScopeMismatch))
            )) => Ok(None),
            // Protected data, backend and lifecycle failures are not corruption.
            Err(error) => Err(error),
        }
    }
}

impl LoadedOperation {
    fn restricted_by(
        &self,
        relays: Option<&RelayProfile>,
        media: Option<BlossomConfigFingerprint>,
    ) -> Result<bool, E> {
        let (targets, _, _) = self.payload.queue_policy().materialize()?;
        let relay_removed = relays.is_some_and(|profile| {
            targets.targets().iter().any(|target| {
                !profile.endpoints().iter().any(|endpoint| {
                    endpoint.access().can_write()
                        && endpoint.url().as_str() == target.uri().as_str()
                })
            })
        });
        Ok(relay_removed
            || media.is_some_and(|current| {
                self.payload
                    .media_policy()
                    .is_some_and(|frozen| &frozen != current.as_bytes())
            }))
    }
}
