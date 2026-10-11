//! Sparse local evidence and artifact custody remain separate from derived rows.
use super::*;
use radroots_storage::event::EventSourceToken;

pub(super) const ID: &str = "tera.today.local.v1";

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct OverlayEvidence {
    pub(super) source_event: String,
    pub(super) overlay: LocalAuthorOverlay,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct LocalState {
    pub(super) schema_version: u16,
    pub(super) context_id: String,
    pub(super) context_generation: u64,
    pub(super) source: EventSourceToken,
    pub(super) generation: ProjectionGeneration,
    pub(super) query_scope: [u8; 32],
    pub(super) overlays: BTreeMap<String, OverlayEvidence>,
    pub(super) media: Vec<([u8; 32], Phase1InboundMediaState)>,
    pub(super) cache: Phase1MediaCacheIndex,
}

impl LocalState {
    pub(super) fn from_state(state: &TodayProjectionState) -> Result<Self, TodayError> {
        let sources = submission_overlay::sources(Some(state));
        let overlays = state
            .overlays
            .iter()
            .map(|(key, overlay)| {
                let id = CardId::parse(key).map_err(|_| TodayError::CorruptProjection)?;
                Ok((
                    key.clone(),
                    OverlayEvidence {
                        source_event: sources
                            .get(&id)
                            .cloned()
                            .ok_or(TodayError::CorruptProjection)?,
                        overlay: overlay.clone(),
                    },
                ))
            })
            .collect::<Result<_, TodayError>>()?;
        Ok(Self {
            schema_version: 1,
            context_id: state.context_id.clone(),
            context_generation: state.context_generation,
            source: state.source_token.ok_or(TodayError::CorruptProjection)?,
            generation: state.row_generation.ok_or(TodayError::CorruptProjection)?,
            query_scope: state.query_scope.ok_or(TodayError::CorruptProjection)?,
            overlays,
            media: local_media_evidence(state).into_iter().collect(),
            cache: state.media_cache.clone(),
        })
    }

    pub(super) fn new(
        context: &LocalNetwork,
        source: EventSourceToken,
        generation: ProjectionGeneration,
        query_scope: [u8; 32],
        prior: Option<Self>,
    ) -> Self {
        let (overlays, media, cache) = prior.map_or_else(
            || {
                (
                    BTreeMap::new(),
                    Vec::new(),
                    Phase1MediaCacheIndex::default(),
                )
            },
            |value| (value.overlays, value.media, value.cache),
        );
        Self {
            schema_version: 1,
            context_id: context.id.as_str().into(),
            context_generation: context.generation,
            source,
            generation,
            query_scope,
            overlays,
            media,
            cache,
        }
    }

    pub(super) fn from_legacy(
        context: &LocalNetwork,
        source: EventSourceToken,
        generation: ProjectionGeneration,
        query_scope: [u8; 32],
        prior: &TodayProjectionState,
    ) -> Result<Self, TodayError> {
        let mut prior = prior.clone();
        prior.row_generation = Some(generation);
        prior.source_token = Some(source);
        prior.query_scope = Some(query_scope);
        let local = Self::from_state(&prior)?;
        Ok(Self::new(
            context,
            source,
            generation,
            query_scope,
            Some(local),
        ))
    }

    pub(super) fn revision(&self) -> Result<u64, TodayError> {
        let digest = Sha256::digest(encode(self)?);
        Ok(u64::from_be_bytes(digest[..8].try_into().expect("digest prefix")).max(1))
    }

    pub(super) fn apply(&self, state: &mut TodayProjectionState) {
        state.media_cache = self.cache.clone();
        for card in &state.cards {
            let key = card.card.card_id.to_hex();
            if let Some(evidence) = self.overlays.get(&key)
                && evidence.source_event == card.card.source_event_id
            {
                state.overlays.insert(key, evidence.overlay.clone());
            }
        }
        apply_local_media_evidence(state, &self.media.iter().cloned().collect());
    }

    pub(super) fn validate(&self, context_key: &str) -> Result<(), TodayError> {
        if self.schema_version != 1 || self.context_id.is_empty() {
            return Err(TodayError::CorruptProjection);
        }
        let mut digest = Sha256::new();
        digest.update(PROJECTION_DOCUMENT_KEY_DOMAIN);
        digest.update(self.context_id.as_bytes());
        digest.update(self.context_generation.to_be_bytes());
        if context_key != format!("context.{}", hex::encode(digest.finalize())) {
            return Err(TodayError::CorruptProjection);
        }
        self.cache.status()?;
        Ok(())
    }
}

pub(super) async fn load(
    storage: &dyn radroots_storage::Storage,
    context: &LocalNetwork,
) -> Result<Option<LocalState>, TodayError> {
    let key = projection_document_key(context);
    let document = ProjectionStore::projection_document(
        storage,
        ProjectionId::parse(ID)?,
        projection_generation()?,
        key.clone(),
    )
    .await?;
    document
        .map(|value| {
            let state: LocalState = decode(value.value())?;
            state.validate(&key)?;
            Ok(state)
        })
        .transpose()
}

pub(super) async fn save(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
    state: &LocalState,
) -> Result<(), TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let key = projection_document_key(context);
    state.validate(&key)?;
    if EventStore::source_token(storage).await? != state.source {
        return Err(CursorError::Stale.into());
    }
    let write = runtime.today_projection_lock.begin_write();
    ProjectionStore::put_projection_document(
        storage,
        ProjectionId::parse(ID)?,
        projection_generation()?,
        ProjectionDocument::new(key, encode(state)?)?,
    )
    .await?;
    write.complete();
    if EventStore::source_token(storage).await? != state.source {
        return Err(CursorError::Stale.into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "today_sidecar_tests.rs"]
mod tests;
