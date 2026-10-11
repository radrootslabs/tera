//! Frozen calendar cursors wrap the shared complete current-read identity.
use super::*;

pub(super) struct Scope {
    read: read_scope::Scope,
    pub(super) cursor: CursorScope,
    pub(super) fingerprint: [u8; 32],
}

impl std::ops::Deref for Scope {
    type Target = read_scope::Scope;
    fn deref(&self) -> &Self::Target {
        &self.read
    }
}

impl Scope {
    pub(super) async fn load(
        runtime: &TeraRuntime,
        context: &LocalNetwork,
        calendar: super::super::ViewerCalendarContext,
        budget: &mut page_budget::Budget,
    ) -> Result<Option<Self>, TodayError> {
        let Some(read) = read_scope::Scope::load(runtime, context, budget).await? else {
            return Ok(None);
        };
        let cursor = CursorScope::new(
            context.id.clone().into(),
            context.generation,
            calendar.as_of(),
            *read.source.generation().as_bytes(),
            read.projection_generation,
            read.local.query_scope,
            calendar,
        )?;
        // Retain the exact v4 fingerprint tuple and cursor encoding.
        let fingerprint = Sha256::digest(encode(&(
            "tera.today-keyset-scope.v4",
            read.source,
            read.local.generation,
            read.local.query_scope,
            read.header.policy,
            read.local_digest,
            &cursor.calendar,
        ))?)
        .into();
        Ok(Some(Self {
            read,
            cursor,
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
