//! Whole-view supporting reader bridge, replaced by selected joins in C203.
//! New builds never invoke this bridge and never persist a copied feed.
use super::*;
use radroots_storage::projection::document_query::{
    ProjectionDocumentGenerations, ProjectionDocumentQuery,
};

pub(super) fn content_generation(
    generation: ProjectionGeneration,
    local: u64,
) -> Result<u64, TodayError> {
    let mut digest = Sha256::new();
    digest.update(b"tera.today.row-content.v1\0");
    digest.update(generation.as_bytes());
    digest.update(local.to_be_bytes());
    let value = digest.finalize();
    Ok(u64::from_be_bytes(value[..8].try_into().expect("digest prefix")).max(1))
}

pub(super) async fn load(
    storage: &dyn radroots_storage::Storage,
    context: &LocalNetwork,
    local: sidecar::LocalState,
) -> Result<Option<TodayProjectionState>, TodayError> {
    let source = EventStore::source_token(storage).await?;
    let id = ProjectionId::parse(build::progress::ID)?;
    let status = ProjectionStore::status(storage, id.clone()).await?;
    if source != local.source
        || status.as_ref().is_none_or(|value| {
            value.generation() != local.generation
                || value.health() != radroots_storage::projection::ProjectionHealth::Ready
        })
    {
        return Ok(None);
    }
    let header = build::progress::load(
        storage,
        id.clone(),
        local.generation,
        build::progress::HEADER,
    )
    .await?
    .ok_or(TodayError::CorruptProjection)?;
    if !header.complete
        || header.source != source
        || header.generation != local.generation
        || header.query_scope != local.query_scope
    {
        return Err(TodayError::CorruptProjection);
    }
    let local_revision = local.revision()?;
    let mut state = TodayProjectionState {
        schema_version: TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION,
        context_id: context.id.as_str().into(),
        context_generation: context.generation,
        store_generation: *source.generation().as_bytes(),
        source_events: header.source_events,
        content_generation: content_generation(local.generation, local_revision)?,
        row_generation: Some(local.generation),
        source_token: Some(source),
        cards: Vec::new(),
        profiles: BTreeMap::new(),
        thread: Vec::new(),
        overlays: BTreeMap::new(),
        media_cache: Phase1MediaCacheIndex::default(),
        query_scope: Some(local.query_scope),
        visibility_digest: Some(crate::runtime::visibility::source_visibility_digest(
            source,
            header.policy,
        )?),
        author_visibility_digest: header.policy,
        quarantined_source_ids: Vec::new(),
    };
    let mut query = ProjectionDocumentQuery::new(
        id,
        ProjectionDocumentGenerations::Exact(local.generation),
        64,
    )?;
    let mut bytes = 0usize;
    loop {
        let page = ProjectionStore::query_projection_documents_at(
            storage,
            query.clone(),
            source,
            local.generation,
        )
        .await?;
        for record in page.records() {
            let document = record.document().ok_or(TodayError::CorruptProjection)?;
            bytes = bytes
                .checked_add(document.value().len())
                .ok_or(radroots_storage::Error::SpaceInsufficient)?;
            if bytes > 64 * 1024 * 1024 {
                return Err(radroots_storage::Error::SpaceInsufficient.into());
            }
            if matches!(
                record.key(),
                build::progress::HEADER | build::progress::PROGRESS
            ) {
                continue;
            }
            if record.key().starts_with(page_index::FIRST) {
                page_index::Reference::from_document(document)?;
                continue;
            }
            match rows::TodayRow::from_document(document)? {
                rows::TodayRow::Card(value) => state.cards.push(value),
                rows::TodayRow::Profile { summary, .. } => {
                    if state
                        .profiles
                        .insert(summary.author_pubkey.clone(), summary)
                        .is_some()
                    {
                        return Err(TodayError::CorruptProjection);
                    }
                }
                rows::TodayRow::Thread(value) => state.thread.push(value),
                rows::TodayRow::Quarantined { event_id } => {
                    state.quarantined_source_ids.push(event_id)
                }
            }
        }
        let Some(cursor) = page.next_cursor() else {
            break;
        };
        query = query.with_cursor(cursor)?;
    }
    let document_rows = header.document_rows()?;
    if state.cards.len() as u64 != header.cards
        || state.profiles.len() as u64 != header.profiles
        || state.thread.len() as u64 != header.threads
        || state.quarantined_source_ids.len() as u64 != header.quarantined
        || status
            .and_then(|value| value.checkpoint().cloned())
            .is_none_or(|value| value.projected_rows() != document_rows)
    {
        return Err(TodayError::CorruptProjection);
    }
    state.cards.sort_by_key(|value| value.card.card_id);
    state
        .thread
        .sort_by(|left, right| left.event_id.cmp(&right.event_id));
    local.apply(&mut state);
    validate_media_state(&state)?;
    if EventStore::source_token(storage).await? != source {
        return Err(CursorError::Stale.into());
    }
    Ok(Some(state))
}
