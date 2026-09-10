// Test-only forwarding adapter. Every operation reaches the real SDK-owned SQLite
// store. Counts describe SPI calls and returned rows, not disabled SQL telemetry.
use radroots_storage::{
    Error, atomic::*, authored::*, authored_atomic::*, authored_delivery::*, authored_draft::*,
    authored_draft_query::*, backup::*, event::*, journal::*, outbox::*, private_artifact::*,
    projection::*, status::*,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) struct QueryCounts(Mutex<std::collections::BTreeMap<&'static str, u64>>);
impl QueryCounts {
    fn add(&self, key: &'static str, amount: u64) {
        *self.0.lock().unwrap().entry(key).or_default() += amount;
    }
    pub(super) fn take(&self) -> std::collections::BTreeMap<&'static str, u64> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}
pub(super) struct CountedStorage {
    pub(super) client: radroots_sdk::Client,
    pub(super) counts: Arc<QueryCounts>,
}
impl CountedStorage {
    fn storage(&self) -> &dyn radroots_storage::Storage {
        self.client.storage().unwrap()
    }
}

impl EventStore for CountedStorage {
    fn status(&self) -> BoxFuture<'_, Result<EventStoreStatus, Error>> {
        Box::pin(async move {
            self.counts.add("event.status", 1);
            EventStore::status(self.storage()).await
        })
    }
    fn admit(&self, admission: EventAdmission) -> BoxFuture<'_, Result<AdmissionReceipt, Error>> {
        Box::pin(async move {
            self.counts.add("event.admit", 1);
            EventStore::admit(self.storage(), admission).await
        })
    }
    fn query_raw(
        &self,
        query: EventQuery,
    ) -> BoxFuture<'_, Result<EventPage<StoredRawEvent>, Error>> {
        Box::pin(async move {
            self.counts.add("event.query_raw", 1);
            let result = EventStore::query_raw(self.storage(), query).await;
            if let Ok(page) = &result {
                self.counts.add("event.rows", page.items().len() as u64);
            }
            result
        })
    }
    fn query_verified(
        &self,
        query: EventQuery,
    ) -> BoxFuture<'_, Result<EventPage<StoredVerifiedEvent>, Error>> {
        Box::pin(async move {
            self.counts.add("event.query_verified", 1);
            let result = EventStore::query_verified(self.storage(), query).await;
            if let Ok(page) = &result {
                self.counts.add("event.rows", page.items().len() as u64);
            }
            result
        })
    }
    fn query_visible(
        &self,
        query: EventQuery,
    ) -> BoxFuture<'_, Result<EventPage<StoredVisibleEvent>, Error>> {
        Box::pin(async move {
            self.counts.add("event.query_visible", 1);
            let result = EventStore::query_visible(self.storage(), query).await;
            if let Ok(page) = &result {
                self.counts.add("event.rows", page.items().len() as u64);
            }
            result
        })
    }
    fn rebuild_visibility(&self) -> BoxFuture<'_, Result<VisibilitySnapshot, Error>> {
        Box::pin(async move {
            self.counts.add("event.rebuild_visibility", 1);
            EventStore::rebuild_visibility(self.storage()).await
        })
    }
    fn query_provenance(
        &self,
        event_id: EventId,
        bounds: EventQueryBounds,
    ) -> BoxFuture<'_, Result<EventPage<StoredEventProvenance>, Error>> {
        Box::pin(async move {
            self.counts.add("event.query_provenance", 1);
            EventStore::query_provenance(self.storage(), event_id, bounds).await
        })
    }
}

impl ProjectionStore for CountedStorage {
    fn status(
        &self,
        projection_id: ProjectionId,
    ) -> BoxFuture<'_, Result<Option<ProjectionStatus>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.status", 1);
            ProjectionStore::status(self.storage(), projection_id).await
        })
    }
    fn checkpoint(
        &self,
        checkpoint: ProjectionCheckpoint,
    ) -> BoxFuture<'_, Result<ProjectionStatus, Error>> {
        Box::pin(async move {
            self.counts.add("projection.checkpoint", 1);
            ProjectionStore::checkpoint(self.storage(), checkpoint).await
        })
    }
    fn invalidate(
        &self,
        invalidation: ProjectionInvalidation,
    ) -> BoxFuture<'_, Result<ProjectionStatus, Error>> {
        Box::pin(async move {
            self.counts.add("projection.invalidate", 1);
            ProjectionStore::invalidate(self.storage(), invalidation).await
        })
    }
    fn invalidation(
        &self,
        projection_id: ProjectionId,
        replacement_generation: ProjectionGeneration,
    ) -> BoxFuture<'_, Result<Option<ProjectionInvalidation>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.invalidation", 1);
            ProjectionStore::invalidation(self.storage(), projection_id, replacement_generation)
                .await
        })
    }
    fn request_rebuild(
        &self,
        ticket: RebuildTicket,
    ) -> BoxFuture<'_, Result<RebuildTicket, Error>> {
        Box::pin(async move {
            self.counts.add("projection.request_rebuild", 1);
            ProjectionStore::request_rebuild(self.storage(), ticket).await
        })
    }
    fn rebuild(
        &self,
        ticket_id: RebuildTicketId,
    ) -> BoxFuture<'_, Result<Option<RebuildTicket>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.rebuild", 1);
            ProjectionStore::rebuild(self.storage(), ticket_id).await
        })
    }
    fn transition_rebuild(
        &self,
        transition: RebuildTransition,
    ) -> BoxFuture<'_, Result<RebuildTicket, Error>> {
        Box::pin(async move {
            self.counts.add("projection.transition_rebuild", 1);
            ProjectionStore::transition_rebuild(self.storage(), transition).await
        })
    }
    fn event_index_manifest(
        &self,
        generation: ProjectionGeneration,
    ) -> BoxFuture<'_, Result<Option<EventIndexManifest>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.event_index_manifest", 1);
            ProjectionStore::event_index_manifest(self.storage(), generation).await
        })
    }
    fn put_event_index_manifest(
        &self,
        manifest: EventIndexManifest,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.counts.add("projection.put_event_index_manifest", 1);
            ProjectionStore::put_event_index_manifest(self.storage(), manifest).await
        })
    }
    fn event_index_checkpoint(
        &self,
        generation: ProjectionGeneration,
    ) -> BoxFuture<'_, Result<Option<EventIndexCheckpoint>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.event_index_checkpoint", 1);
            ProjectionStore::event_index_checkpoint(self.storage(), generation).await
        })
    }
    fn put_event_index_checkpoint(
        &self,
        checkpoint: EventIndexCheckpoint,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.counts.add("projection.put_event_index_checkpoint", 1);
            ProjectionStore::put_event_index_checkpoint(self.storage(), checkpoint).await
        })
    }
    fn put_projection_document(
        &self,
        projection_id: ProjectionId,
        generation: ProjectionGeneration,
        document: ProjectionDocument,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.counts.add("projection.put_projection_document", 1);
            ProjectionStore::put_projection_document(
                self.storage(),
                projection_id,
                generation,
                document,
            )
            .await
        })
    }
    fn projection_document(
        &self,
        projection_id: ProjectionId,
        generation: ProjectionGeneration,
        key: String,
    ) -> BoxFuture<'_, Result<Option<ProjectionDocument>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.projection_document", 1);
            ProjectionStore::projection_document(self.storage(), projection_id, generation, key)
                .await
        })
    }
    fn put_projection_snapshot(
        &self,
        snapshot: ProjectionSnapshot,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.counts.add("projection.put_projection_snapshot", 1);
            ProjectionStore::put_projection_snapshot(self.storage(), snapshot).await
        })
    }
    fn projection_snapshot(
        &self,
        projection_id: ProjectionId,
        snapshot_id: [u8; 32],
    ) -> BoxFuture<'_, Result<Option<ProjectionSnapshot>, Error>> {
        Box::pin(async move {
            self.counts.add("projection.projection_snapshot", 1);
            ProjectionStore::projection_snapshot(self.storage(), projection_id, snapshot_id).await
        })
    }
}

impl Journal for CountedStorage {
    fn prepare(&self, operation: PrepareOperation) -> BoxFuture<'_, Result<PrepareReceipt, Error>> {
        Journal::prepare(self.storage(), operation)
    }
    fn operation(
        &self,
        instance_id: OperationInstanceId,
    ) -> BoxFuture<'_, Result<Option<OperationRecord>, Error>> {
        Journal::operation(self.storage(), instance_id)
    }
    fn by_idempotency_key(
        &self,
        operation_id: OperationId,
        idempotency_key: IdempotencyKey,
    ) -> BoxFuture<'_, Result<Option<OperationRecord>, Error>> {
        Journal::by_idempotency_key(self.storage(), operation_id, idempotency_key)
    }
    fn transition(
        &self,
        transition: JournalTransition,
    ) -> BoxFuture<'_, Result<OperationRecord, Error>> {
        Journal::transition(self.storage(), transition)
    }
    fn recoverable(&self, limit: u16) -> BoxFuture<'_, Result<Vec<OperationRecord>, Error>> {
        Journal::recoverable(self.storage(), limit)
    }
}

impl Outbox for CountedStorage {
    fn enqueue(&self, item: EnqueueOutboxItem) -> BoxFuture<'_, Result<EnqueueReceipt, Error>> {
        Outbox::enqueue(self.storage(), item)
    }
    fn item(&self, item_id: OutboxItemId) -> BoxFuture<'_, Result<Option<OutboxRecord>, Error>> {
        Outbox::item(self.storage(), item_id)
    }
    fn claim(
        &self,
        request: ClaimOutboxItems,
    ) -> BoxFuture<'_, Result<Vec<ClaimedOutboxItem>, Error>> {
        Outbox::claim(self.storage(), request)
    }
    fn record_attempt(
        &self,
        evidence: DeliveryAttemptEvidence,
    ) -> BoxFuture<'_, Result<OutboxRecord, Error>> {
        Outbox::record_attempt(self.storage(), evidence)
    }
    fn release(
        &self,
        item_id: OutboxItemId,
        lease_id: LeaseId,
        expected_revision: OutboxRevision,
        released_at_unix_ms: u64,
        retry_not_before_unix_ms: Option<u64>,
    ) -> BoxFuture<'_, Result<OutboxRecord, Error>> {
        Outbox::release(
            self.storage(),
            item_id,
            lease_id,
            expected_revision,
            released_at_unix_ms,
            retry_not_before_unix_ms,
        )
    }
    fn status(&self) -> BoxFuture<'_, Result<OutboxStatus, Error>> {
        Outbox::status(self.storage())
    }
}

impl PrivateArtifactStore for CountedStorage {
    fn put_metadata(
        &self,
        metadata: PrivateArtifactMetadata,
    ) -> BoxFuture<'_, Result<PrivateArtifactMetadata, Error>> {
        PrivateArtifactStore::put_metadata(self.storage(), metadata)
    }
    fn metadata(
        &self,
        artifact_id: PrivateArtifactId,
    ) -> BoxFuture<'_, Result<Option<PrivateArtifactMetadata>, Error>> {
        PrivateArtifactStore::metadata(self.storage(), artifact_id)
    }
    fn reseal_metadata(
        &self,
        request: PrivateArtifactResealRequest,
    ) -> BoxFuture<'_, Result<PrivateArtifactResealReceipt, Error>> {
        PrivateArtifactStore::reseal_metadata(self.storage(), request)
    }
    fn mark_expired(
        &self,
        artifact_id: PrivateArtifactId,
        expected_revision: PrivateArtifactRevision,
        at_unix_ms: u64,
    ) -> BoxFuture<'_, Result<PrivateArtifactMetadata, Error>> {
        PrivateArtifactStore::mark_expired(
            self.storage(),
            artifact_id,
            expected_revision,
            at_unix_ms,
        )
    }
    fn tombstone(
        &self,
        artifact_id: PrivateArtifactId,
        expected_revision: PrivateArtifactRevision,
        at_unix_ms: u64,
        reason: DeletionReason,
    ) -> BoxFuture<'_, Result<PrivateArtifactMetadata, Error>> {
        PrivateArtifactStore::tombstone(
            self.storage(),
            artifact_id,
            expected_revision,
            at_unix_ms,
            reason,
        )
    }
    fn expired(
        &self,
        at_unix_ms: u64,
        limit: u16,
    ) -> BoxFuture<'_, Result<Vec<PrivateArtifactMetadata>, Error>> {
        PrivateArtifactStore::expired(self.storage(), at_unix_ms, limit)
    }
    fn status(&self) -> BoxFuture<'_, Result<PrivateArtifactStatus, Error>> {
        PrivateArtifactStore::status(self.storage())
    }
}

impl StorageReliability for CountedStorage {
    fn begin_backup(&self, plan: BackupPlan) -> BoxFuture<'_, Result<BackupOperation, Error>> {
        StorageReliability::begin_backup(self.storage(), plan)
    }
    fn transition_backup(
        &self,
        backup_id: BackupId,
        expected_revision: ReliabilityRevision,
        transition: BackupTransition,
        at_unix_ms: u64,
    ) -> BoxFuture<'_, Result<BackupOperation, Error>> {
        StorageReliability::transition_backup(
            self.storage(),
            backup_id,
            expected_revision,
            transition,
            at_unix_ms,
        )
    }
    fn begin_restore(&self, plan: RestorePlan) -> BoxFuture<'_, Result<RestoreOperation, Error>> {
        StorageReliability::begin_restore(self.storage(), plan)
    }
    fn transition_restore(
        &self,
        backup_id: BackupId,
        expected_revision: ReliabilityRevision,
        transition: RestoreTransition,
        at_unix_ms: u64,
    ) -> BoxFuture<'_, Result<RestoreOperation, Error>> {
        StorageReliability::transition_restore(
            self.storage(),
            backup_id,
            expected_revision,
            transition,
            at_unix_ms,
        )
    }
    fn integrity(&self) -> BoxFuture<'_, Result<IntegrityStatus, Error>> {
        StorageReliability::integrity(self.storage())
    }
    fn status(&self) -> BoxFuture<'_, Result<StorageStatus, Error>> {
        StorageReliability::status(self.storage())
    }
    fn close(&self) -> BoxFuture<'_, Result<StorageStatus, Error>> {
        StorageReliability::close(self.storage())
    }
}

impl AtomicStorage for CountedStorage {
    fn commit(&self, request: AtomicCommit) -> BoxFuture<'_, Result<AtomicCommitReceipt, Error>> {
        AtomicStorage::commit(self.storage(), request)
    }
    fn receipt(
        &self,
        commit_id: AtomicCommitId,
    ) -> BoxFuture<'_, Result<Option<AtomicCommitReceipt>, Error>> {
        AtomicStorage::receipt(self.storage(), commit_id)
    }
}

impl AuthoredAtomicStorage for CountedStorage {
    fn execute_authored(
        &self,
        command: AuthoredAtomicCommand,
    ) -> BoxFuture<'_, Result<AuthoredAtomicReceipt, Error>> {
        AuthoredAtomicStorage::execute_authored(self.storage(), command)
    }
    fn authored_receipt(
        &self,
        commit_id: AtomicCommitId,
    ) -> BoxFuture<'_, Result<Option<AuthoredAtomicReceipt>, Error>> {
        AuthoredAtomicStorage::authored_receipt(self.storage(), commit_id)
    }
    fn authored_operation(
        &self,
        operation_id: OperationInstanceId,
    ) -> BoxFuture<'_, Result<Option<AuthoredOperation>, Error>> {
        AuthoredAtomicStorage::authored_operation(self.storage(), operation_id)
    }
    fn authored_artifact(
        &self,
        artifact_id: AuthoredArtifactId,
    ) -> BoxFuture<'_, Result<Option<AuthoredArtifact>, Error>> {
        AuthoredAtomicStorage::authored_artifact(self.storage(), artifact_id)
    }
    fn authored_delivery_plan(
        &self,
        plan_id: AuthoredDeliveryPlanId,
    ) -> BoxFuture<'_, Result<Option<AuthoredDeliveryPlan>, Error>> {
        AuthoredAtomicStorage::authored_delivery_plan(self.storage(), plan_id)
    }
}

impl AuthoredDraftStore for CountedStorage {
    fn query_authored_drafts(
        &self,
        query: AuthoredDraftQuery,
    ) -> BoxFuture<'_, Result<AuthoredDraftPage, Error>> {
        AuthoredDraftStore::query_authored_drafts(self.storage(), query)
    }
    fn append_authored_draft(
        &self,
        draft: AuthoredDraft,
        expected_head: Option<AuthoredDraftRevision>,
    ) -> BoxFuture<'_, Result<DraftAppendReceipt, Error>> {
        AuthoredDraftStore::append_authored_draft(self.storage(), draft, expected_head)
    }
    fn authored_draft_head(
        &self,
        draft_id: AuthoredDraftId,
    ) -> BoxFuture<'_, Result<Option<AuthoredDraft>, Error>> {
        AuthoredDraftStore::authored_draft_head(self.storage(), draft_id)
    }
    fn authored_draft_revision(
        &self,
        draft_id: AuthoredDraftId,
        revision: AuthoredDraftRevision,
    ) -> BoxFuture<'_, Result<Option<AuthoredDraft>, Error>> {
        AuthoredDraftStore::authored_draft_revision(self.storage(), draft_id, revision)
    }
    fn authored_draft_heads(
        &self,
        author: [u8; 32],
        limit: u16,
    ) -> BoxFuture<'_, Result<Vec<AuthoredDraft>, Error>> {
        AuthoredDraftStore::authored_draft_heads(self.storage(), author, limit)
    }
}
