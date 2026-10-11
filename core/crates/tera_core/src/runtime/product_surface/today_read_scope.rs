//! One complete bounded read identity for every current product surface.
use super::*;
use radroots_storage::event::EventSourceToken;
use radroots_storage::projection::ProjectionHealth;

pub(super) struct Scope {
    pub(super) projection_generation: u64,
    pub(super) local_digest: [u8; 32],
    pub(super) source: EventSourceToken,
    pub(super) local: sidecar::LocalState,
    pub(super) header: build::progress::BuildProgress,
    pub(super) fingerprint: [u8; 32],
}

impl Scope {
    pub(super) async fn load(
        runtime: &TeraRuntime,
        context: &LocalNetwork,
        budget: &mut page_budget::Budget,
    ) -> Result<Option<Self>, TodayError> {
        Self::load_with_context(runtime, context, budget, false).await
    }

    pub(super) async fn load_matching_context(
        runtime: &TeraRuntime,
        context: &LocalNetwork,
        budget: &mut page_budget::Budget,
    ) -> Result<Option<Self>, TodayError> {
        Self::load_with_context(runtime, context, budget, true).await
    }

    async fn load_with_context(
        runtime: &TeraRuntime,
        context: &LocalNetwork,
        budget: &mut page_budget::Budget,
        require_matching_context: bool,
    ) -> Result<Option<Self>, TodayError> {
        let storage = runtime
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        if !calendar_ready(storage, budget).await? {
            return Ok(None);
        }
        budget.charge(512, 1)?;
        let query_scope = paging_scope::query_scope(context, runtime.store_public_key)?;
        let source = EventStore::source_token(storage).await?;
        if !runtime.author_visibility_fence.can_collect() {
            return Err(TodayError::RuntimeUnavailable);
        }
        let (id, generation) = crate::runtime::visibility::identity()?;
        let policy_document = budget
            .document(
                storage,
                id,
                generation,
                crate::runtime::visibility::KEY.into(),
                None,
            )
            .await?;
        let policy =
            crate::runtime::visibility::decode_policy(policy_document.as_ref())?.cache_digest()?;
        if !runtime.author_visibility_fence.can_collect() {
            return Err(TodayError::RuntimeUnavailable);
        }
        let key = projection_document_key(context);
        let Some(document) = budget
            .document(
                storage,
                ProjectionId::parse(sidecar::ID)?,
                projection_generation()?,
                key.clone(),
                None,
            )
            .await?
        else {
            return Ok(None);
        };
        let local: sidecar::LocalState = decode(document.value())?;
        local.validate(&key)?;
        if require_matching_context && local.query_scope != query_scope {
            return Err(CursorError::Stale.into());
        }
        if local.source != source || local.query_scope != query_scope {
            return Ok(None);
        }
        let id = ProjectionId::parse(build::progress::ID)?;
        let status = ProjectionStore::status(storage, id.clone()).await?;
        if status.as_ref().is_none_or(|status| {
            status.generation() != local.generation || status.health() != ProjectionHealth::Ready
        }) {
            return Ok(None);
        }
        let Some(header) = budget
            .document(
                storage,
                id,
                local.generation,
                build::progress::HEADER.into(),
                Some(source),
            )
            .await?
        else {
            return Err(TodayError::CorruptProjection);
        };
        let header: build::progress::BuildProgress = decode(header.value())?;
        if !header.complete
            || header.source != source
            || header.generation != local.generation
            || header.query_scope != query_scope
        {
            return Err(TodayError::CorruptProjection);
        }
        if header.index_schema != page_index::SCHEMA || header.policy != policy {
            return Ok(None);
        }
        if header.index_rows < header.cards
            || header.index_rows
                > header
                    .cards
                    .checked_mul(4)
                    .ok_or(TodayError::CorruptProjection)?
        {
            return Err(TodayError::CorruptProjection);
        }
        if status
            .and_then(|status| status.checkpoint().cloned())
            .is_none_or(|checkpoint| {
                checkpoint.projected_rows() != header.document_rows().unwrap_or(u64::MAX)
            })
        {
            return Err(TodayError::CorruptProjection);
        }
        let local_digest: [u8; 32] = Sha256::digest(document.value()).into();
        let revision =
            u64::from_be_bytes(local_digest[..8].try_into().expect("digest prefix")).max(1);
        let projection_generation = row_state::content_generation(local.generation, revision)?;
        let fingerprint = Sha256::digest(encode(&(
            "tera.today-read-scope.v1",
            source,
            local.generation,
            query_scope,
            policy,
            local_digest,
        ))?)
        .into();
        budget.charge(512, 1)?;
        if EventStore::source_token(storage).await? != source {
            return Err(CursorError::Stale.into());
        }
        Ok(Some(Self {
            projection_generation,
            local_digest,
            source,
            local,
            header,
            fingerprint,
        }))
    }

    /// Whole-view supporting hydration remains until the separately ordered
    /// selected-query checkpoint. It never changes canonical source authority.
    pub(super) async fn load_state(
        &self,
        runtime: &TeraRuntime,
        context: &LocalNetwork,
    ) -> Result<TodayProjectionState, TodayError> {
        let storage = runtime
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let state = row_state::load(storage, context, self.local.clone())
            .await
            .map_err(current_read_error)?
            .ok_or(CursorError::Stale)?;
        self.check_state(&state)?;
        Ok(state)
    }

    pub(super) fn check_state(&self, state: &TodayProjectionState) -> Result<(), TodayError> {
        if state.source_token != Some(self.source)
            || state.store_generation != *self.source.generation().as_bytes()
            || state.row_generation != Some(self.local.generation)
            || state.query_scope != Some(self.local.query_scope)
            || state.author_visibility_digest != self.header.policy
            || state.visibility_digest
                != Some(crate::runtime::visibility::source_visibility_digest(
                    self.source,
                    self.header.policy,
                )?)
            || state.content_generation != self.projection_generation
        {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }

    pub(super) async fn check_current(
        &self,
        runtime: &TeraRuntime,
        context: &LocalNetwork,
        budget: &mut page_budget::Budget,
    ) -> Result<(), TodayError> {
        let current = Self::load(runtime, context, budget)
            .await?
            .ok_or(CursorError::Stale)?;
        if current.fingerprint != self.fingerprint {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }
}

async fn calendar_ready(
    storage: &dyn radroots_storage::Storage,
    budget: &mut page_budget::Budget,
) -> Result<bool, TodayError> {
    let Some(status) = ProjectionStore::status(storage, projection_id()?).await? else {
        return Ok(true);
    };
    if status.generation() == calendar_migration::legacy_generation()? {
        return Ok(false);
    }
    if status.generation() != projection_generation()? {
        return Err(TodayError::UnsupportedProjectionVersion);
    }
    if status.health() == ProjectionHealth::Ready {
        return Ok(true);
    }
    let (id, generation, key) = coordination_retirement::document_identity()?;
    let document = budget.document(storage, id, generation, key, None).await?;
    coordination_retirement::retired_document(storage, &status, document.as_ref()).await
}

/// A proven conditional-read conflict invalidates retained current content.
/// Ordinary unavailable storage still permits the host's voluntary cache policy.
pub(super) fn current_read_error(error: TodayError) -> TodayError {
    match error {
        TodayError::Storage(
            radroots_storage::Error::SourceEpochChanged
            | radroots_storage::Error::ProjectionRevisionConflict,
        ) => CursorError::Stale.into(),
        error => error,
    }
}

/// Called only after the reader has proved that its prior scope is unavailable.
/// Capacity keeps its existing recovery report; other failed storage repair
/// cannot authorize the already superseded presentation as voluntary cache.
pub(super) fn current_repair_error(error: TodayError) -> TodayError {
    match current_read_error(error) {
        error @ TodayError::Storage(
            radroots_storage::Error::SpaceInsufficient
            | radroots_storage::Error::SourceGenerationChanged,
        ) => error,
        TodayError::Storage(_) => TodayError::ProjectionMissing,
        error => error,
    }
}
