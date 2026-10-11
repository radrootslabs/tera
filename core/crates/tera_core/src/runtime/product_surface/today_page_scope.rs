//! Read only a small promoted header and bounded sparse local evidence.
use super::*;
use radroots_storage::event::EventSourceToken;
use radroots_storage::projection::ProjectionHealth;

pub(super) struct Scope {
    pub(super) cursor: CursorScope,
    pub(super) source: EventSourceToken,
    pub(super) local: sidecar::LocalState,
    pub(super) header: build::progress::BuildProgress,
    pub(super) fingerprint: [u8; 32],
}

impl Scope {
    pub(super) async fn load(
        runtime: &TeraRuntime,
        context: &LocalNetwork,
        calendar: super::super::ViewerCalendarContext,
        budget: &mut page_budget::Budget,
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
        let cursor = CursorScope::new(
            context.id.clone().into(),
            context.generation,
            calendar.as_of(),
            *source.generation().as_bytes(),
            row_state::content_generation(local.generation, revision)?,
            query_scope,
            calendar,
        )?;
        let fingerprint = Sha256::digest(encode(&(
            "tera.today-keyset-scope.v4",
            source,
            local.generation,
            query_scope,
            policy,
            local_digest,
            &cursor.calendar,
        ))?)
        .into();
        Ok(Some(Self {
            cursor,
            source,
            local,
            header,
            fingerprint,
        }))
    }

    pub(super) fn check_cursor(&self, decoded: &page_cursor::Decoded) -> Result<(), TodayError> {
        TodayCursor::decode(&decoded.inner, &self.cursor)?;
        if decoded.fingerprint != self.fingerprint {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }

    pub(super) fn state(&self, context: &LocalNetwork) -> TodayProjectionState {
        TodayProjectionState {
            schema_version: TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION,
            context_id: context.id.clone().into(),
            context_generation: context.generation,
            store_generation: *self.source.generation().as_bytes(),
            source_events: self.header.source_events,
            content_generation: self.cursor.projection_generation,
            row_generation: Some(self.local.generation),
            source_token: Some(self.source),
            cards: Vec::new(),
            profiles: BTreeMap::new(),
            thread: Vec::new(),
            overlays: BTreeMap::new(),
            media_cache: Phase1MediaCacheIndex::default(),
            query_scope: Some(self.cursor.query_scope),
            visibility_digest: None,
            author_visibility_digest: self.header.policy,
            quarantined_source_ids: Vec::new(),
        }
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
