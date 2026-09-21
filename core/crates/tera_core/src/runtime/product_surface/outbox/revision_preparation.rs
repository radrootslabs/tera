//! Immutable preparation identity is separate from subsequent child progress.

use super::*;

impl TeraRuntime {
    /// Replays the original captured replacement after an uncertain response.
    /// A new intentional revision requires a fresh nonzero request identity.
    pub async fn prepare_revision_intent(
        &self,
        request_id: [u8; 16],
        intent: Phase1ReviseIntent,
    ) -> Result<Phase1RevisionStatus, Phase1DraftError> {
        Box::pin(self.prepare_revision_intent_with_clock(
            request_id,
            intent,
            phase1_operation_now_unix_ms,
        ))
        .await
    }

    pub(super) async fn prepare_revision_intent_with_clock(
        &self,
        request_id: [u8; 16],
        intent: Phase1ReviseIntent,
        clock: impl Fn() -> Result<u64, Phase1DraftError> + Send + Sync,
    ) -> Result<Phase1RevisionStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        let author = self.draft_author()?;
        if request_id == [0; 16] || intent.target.author_public_key != hex::encode(author) {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let draft_id = revision_id(b"tera.revision_replacement.v1\0", author, request_id)?;
        let _admission = self.mutations.draft(*draft_id.as_bytes())?;
        let storage = self.storage()?;
        // The immutable first revision remains the comparison authority after
        // media progress, queueing, signing, cancellation or child creation.
        let original = storage
            .authored_draft_revision(draft_id, AuthoredDraftRevision::INITIAL)
            .await
            .map_err(|_| Phase1DraftError::Storage)?;
        let now = match &original {
            Some(original) => original.created_at_unix_ms(),
            None => clock()?,
        };
        let plan = intent
            .command
            .authored_plan(now / 1_000, hex::encode(author))
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let wire = PlanWireV1::from_plan(&plan)
            .to_json()
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let policy = if intent.target.source_kind == 1 {
            Phase1RevisionPolicy::ReplaceThenRetract
        } else {
            Phase1RevisionPolicy::AddressableReplacement
        };
        let child = match policy {
            Phase1RevisionPolicy::ReplaceThenRetract => {
                Some(*revision_id(b"tera.revision_retraction.v1\0", author, request_id)?.as_bytes())
            }
            Phase1RevisionPolicy::AddressableReplacement => None,
        };
        if child == Some(*draft_id.as_bytes()) {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let mut payload =
            Phase1DraftPayload::new(&intent.command, wire, intent.media, Some(intent.form))?;
        payload.revision = Some(Phase1RevisionRecord {
            target: intent.target,
            policy,
            retraction_draft_id: child,
        });
        let candidate = AuthoredDraft::initial(
            draft_id,
            author,
            DRAFT_PAYLOAD_SCHEMA,
            payload.encode()?,
            draft_stage_for_media(&payload.media),
            None,
            now,
        )
        .map_err(|_| Phase1DraftError::InvalidDraft)?;
        if let Some(original) = original {
            if original != candidate {
                return Err(Phase1DraftError::RevisionConflict);
            }
        } else {
            if policy == Phase1RevisionPolicy::ReplaceThenRetract {
                self.require_revision_source(
                    &payload
                        .revision
                        .as_ref()
                        .ok_or(Phase1DraftError::Corrupt)?
                        .target,
                )
                .await?;
            }
            let receipt = storage
                .append_authored_draft(candidate.clone(), None)
                .await
                .map_err(map_draft_storage_error)?;
            if receipt.draft() != &candidate {
                return Err(Phase1DraftError::Corrupt);
            }
            // Captured input alone is not publication permission. Cancellation
            // here leaves retained input; claim and binding still commit together.
            let _coordinate = self.admit_coordinate(&candidate).await?;
        }
        self.phase1_revision_status(*draft_id.as_bytes()).await
    }
}

fn revision_id(
    domain: &[u8],
    author: [u8; 32],
    request: [u8; 16],
) -> Result<AuthoredDraftId, Phase1DraftError> {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(author);
    hash.update(request);
    let mut id = [0; 16];
    id.copy_from_slice(&hash.finalize()[..16]);
    AuthoredDraftId::new(id).map_err(|_| Phase1DraftError::InvalidRevision)
}
