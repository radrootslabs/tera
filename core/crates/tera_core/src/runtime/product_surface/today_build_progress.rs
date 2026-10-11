//! Small acknowledged progress; no retained source/card/thread collection.
use super::*;
use radroots_storage::event::{EventCursor, EventSourceToken};
use radroots_storage::projection::batch::ProjectionWriteScope;

pub(in super::super) const ID: &str = "tera.today.rows.v1";
pub(in super::super) const PROGRESS: &str = "0.progress";
pub(in super::super) const HEADER: &str = "0.header";
const POINTER: &str = "today.rows.staging";
const POINTER_ID: &str = "tera.today.build.v1";

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(in super::super) struct BuildProgress {
    pub(in super::super) source: EventSourceToken,
    pub(in super::super) query_scope: [u8; 32],
    pub(in super::super) policy: Option<[u8; 32]>,
    pub(in super::super) generation: ProjectionGeneration,
    pub(super) expected_active: Option<ProjectionGeneration>,
    pub(super) after: Option<EventCursor>,
    pub(in super::super) source_events: u64,
    pub(in super::super) cards: u64,
    pub(in super::super) profiles: u64,
    pub(in super::super) threads: u64,
    pub(in super::super) quarantined: u64,
    pub(in super::super) complete: bool,
}

impl BuildProgress {
    pub(super) fn new(
        source: EventSourceToken,
        query_scope: [u8; 32],
        policy: Option<[u8; 32]>,
        active: Option<ProjectionGeneration>,
        source_events: u64,
    ) -> Result<Self, TodayError> {
        let mut digest = Sha256::new();
        digest.update(b"tera.today.row-generation.v1\0");
        digest.update(encode(&(source, query_scope, policy))?);
        digest.update(uuid::Uuid::new_v4().as_bytes());
        Ok(Self {
            source,
            query_scope,
            policy,
            generation: ProjectionGeneration::new(digest.finalize().into())?,
            expected_active: active,
            after: None,
            source_events,
            cards: 0,
            profiles: 0,
            threads: 0,
            quarantined: 0,
            complete: false,
        })
    }
    pub(super) fn scope(&self) -> Result<ProjectionWriteScope, TodayError> {
        Ok(ProjectionWriteScope::new(
            ProjectionId::parse(ID)?,
            self.generation,
            self.source,
            self.expected_active,
        )?)
    }
    pub(super) fn count(&mut self, row: &rows::TodayRow) -> Result<(), TodayError> {
        let count = match row {
            rows::TodayRow::Card(_) => &mut self.cards,
            rows::TodayRow::Profile { .. } => &mut self.profiles,
            rows::TodayRow::Thread(_) => &mut self.threads,
            rows::TodayRow::Quarantined { .. } => &mut self.quarantined,
        };
        *count = count.checked_add(1).ok_or(TodayError::CorruptProjection)?;
        Ok(())
    }
    pub(in super::super) fn document_rows(&self) -> Result<u64, TodayError> {
        [self.cards, self.profiles, self.threads, self.quarantined]
            .into_iter()
            .try_fold(2u64, |total, value| {
                total
                    .checked_add(value)
                    .ok_or(TodayError::CorruptProjection)
            })
    }
    pub(in super::super) fn receipt(
        &self,
        update: TodayProjectionUpdate,
        changed: bool,
    ) -> TodayRefreshReceipt {
        TodayRefreshReceipt {
            update,
            source_events: self.source_events,
            visible_cards: self.cards,
            profiles: self.profiles,
            thread_entries: self.threads,
            content_generation: u64::from_be_bytes(
                self.generation.as_bytes()[..8]
                    .try_into()
                    .expect("digest prefix"),
            )
            .max(1),
            changed,
        }
    }
}

pub(super) async fn pointer(
    storage: &dyn radroots_storage::Storage,
) -> Result<Option<BuildProgress>, TodayError> {
    load(
        storage,
        ProjectionId::parse(POINTER_ID)?,
        projection_generation()?,
        POINTER,
    )
    .await
}
pub(super) async fn save_pointer(
    runtime: &TeraRuntime,
    progress: &BuildProgress,
) -> Result<(), TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let write = runtime.today_projection_lock.begin_write();
    ProjectionStore::put_projection_document(
        storage,
        ProjectionId::parse(POINTER_ID)?,
        projection_generation()?,
        ProjectionDocument::new(POINTER.into(), encode(progress)?)?,
    )
    .await?;
    write.complete();
    Ok(())
}
pub(in super::super) async fn load(
    storage: &dyn radroots_storage::Storage,
    id: ProjectionId,
    generation: ProjectionGeneration,
    key: &str,
) -> Result<Option<BuildProgress>, TodayError> {
    let document =
        ProjectionStore::projection_document(storage, id, generation, key.into()).await?;
    document.map(|value| decode(value.value())).transpose()
}
