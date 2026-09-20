use std::sync::Arc;

use radroots_blossom::{BlobUrl, MediaType, Sha256 as BlobHash, descriptor::ByteCommitment};

use super::super::tests::context;
use super::*;

#[path = "today_media_collection_budget_tests.rs"]
mod budgets;

struct Fixture {
    root: tempfile::TempDir,
    runtime: Arc<TeraRuntime>,
    empty: TodayProjectionState,
    receipt: Phase1VerifiedMediaReceipt,
    bytes: Vec<u8>,
}

impl Fixture {
    async fn new() -> Self {
        Self::backend(false).await
    }

    async fn backend(sqlite: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = if sqlite {
            use crate::runtime::{
                builder::RuntimeBuilder,
                store::{MobileUserStoreConfig, ProtectedDataAvailability},
            };
            let config = MobileUserStoreConfig::from_encoded(
                root.path(),
                &super::super::tests::keys().public_key().to_string(),
                &"87".repeat(32),
                2_000_000_000_000,
                ProtectedDataAvailability::Available,
            )
            .unwrap();
            std::fs::create_dir_all(config.owner_directory()).unwrap();
            RuntimeBuilder::new(config).build().await.unwrap()
        } else {
            TeraRuntime::test_memory().unwrap()
        };
        runtime.inbound_media_directory = Some(root.path().join("inbound_media.v1"));
        let selected = context(None, 1);
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(20, 2_000_000_200, "UTC"))
            .await
            .unwrap();
        let empty = load_state(
            runtime.client.storage().unwrap(),
            &selected,
            projection_generation().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        let bytes = b"cache ownership fixture".to_vec();
        let hash = BlobHash::digest(&bytes).to_hex();
        let url = format!("https://media.example/{hash}.png");
        let reference = Phase1StructuralMediaReference::new(
            &url,
            Some(hash),
            Some("image/png".into()),
            Some(2),
            Some(3),
            Some(bytes.len() as u64),
            None,
        )
        .unwrap();
        let receipt = Phase1VerifiedMediaReceipt::from_commitment(
            &reference,
            BlobUrl::parse(&url).unwrap(),
            &ByteCommitment::from_bytes(&bytes, MediaType::parse("image/png").unwrap()),
            2,
            3,
            Phase1MediaConfigurationFingerprint::new([9; 32]).unwrap(),
            10,
        )
        .unwrap();
        Self {
            root,
            runtime: Arc::new(runtime),
            empty,
            receipt,
            bytes,
        }
    }

    fn directory(&self) -> &Path {
        self.runtime.inbound_media_directory.as_deref().unwrap()
    }

    fn path(&self) -> std::path::PathBuf {
        self.directory()
            .join(format!("{}.png", self.receipt.artifact_id().to_hex()))
    }

    async fn file(&self) {
        super::super::super::media::write_verified_artifact(
            self.directory(),
            &self.receipt,
            &self.bytes,
        )
        .await
        .unwrap();
    }

    async fn state(&self, generation: u64, owned: bool) {
        let selected = context(None, generation);
        let mut state = self.empty.clone();
        state.context_generation = generation;
        if owned {
            state
                .media_cache
                .admit(&self.receipt, Phase1MediaCachePolicy::default(), 11)
                .unwrap();
        }
        persist_media_state(
            &self.runtime,
            self.runtime.client.storage().unwrap(),
            &selected,
            projection_generation().unwrap(),
            &mut state,
        )
        .await
        .unwrap();
    }

    async fn candidates(&self) -> Option<BTreeSet<Phase1MediaArtifactId>> {
        unreferenced(
            self.runtime.client.storage().unwrap(),
            &[self.receipt.artifact_id()],
        )
        .await
        .unwrap()
    }

    async fn collect(&self) -> Result<(), TodayError> {
        let files = self.runtime.inbound_media_lock.lock().await;
        let projection = self.runtime.today_projection_lock.lock().await;
        collect(
            &self.runtime,
            self.directory(),
            &[self.receipt.artifact_id()],
            &files,
            &projection,
        )
        .await
    }
}

#[tokio::test]
async fn invalidating_one_context_preserves_another_then_last_owner_allows_cleanup() {
    for sqlite in [false, true] {
        shared_context_cleanup(Fixture::backend(sqlite).await).await;
    }
}

async fn shared_context_cleanup(f: Fixture) {
    f.file().await;
    f.state(1, true).await;
    f.state(2, true).await;
    assert!(
        f.runtime
            .phase1_invalidate_media_artifact(&context(None, 1), f.receipt.artifact_id())
            .await
            .unwrap()
    );
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
    assert_eq!(
        f.runtime
            .phase1_media_cache_status(&context(None, 2))
            .await
            .unwrap()
            .artifacts,
        1
    );
    f.runtime
        .phase1_invalidate_media_configuration(
            &context(None, 2),
            Phase1MediaConfigurationFingerprint::new([8; 32]).unwrap(),
        )
        .await
        .unwrap();
    assert!(!f.path().exists());
    f.collect().await.unwrap(); // Interrupted/repeated cleanup is idempotent.
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn complete_scan_reaches_owner_beyond_one_thousand_records() {
    let f = Fixture::new().await;
    for generation in 1..=1_025 {
        f.state(generation, false).await;
    }
    // Select the last ordered document, not the last insertion.
    let selected = (1..=1_025)
        .max_by_key(|g| projection_document_key(&context(None, *g)))
        .unwrap();
    f.state(selected, true).await;
    assert!(f.candidates().await.unwrap().is_empty());
    f.state(selected, false).await;
    assert_eq!(
        f.candidates().await.unwrap(),
        BTreeSet::from([f.receipt.artifact_id()])
    );
}

#[tokio::test]
async fn exhausted_inventory_budget_retains_unreferenced_bytes() {
    let f = Fixture::new().await;
    for generation in 1..=(MAX_PAGES * usize::from(PAGE_ROWS) + 1) as u64 {
        f.state(generation, false).await;
    }
    f.file().await;
    assert!(f.candidates().await.is_none());
    f.collect().await.unwrap();
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
}

#[tokio::test]
async fn unknown_generation_corrupt_and_noncanonical_documents_never_authorize_deletion() {
    for kind in 0..4 {
        let f = Fixture::new().await;
        f.file().await;
        let mut bytes = encode(&f.empty).unwrap();
        let mut generation = projection_generation().unwrap();
        let mut key = projection_document_key(&context(None, 1));
        match kind {
            0 => generation = ProjectionGeneration::new([55; 32]).unwrap(),
            1 => bytes = b"corrupt".to_vec(),
            2 => {
                bytes.pop();
                bytes.extend_from_slice(b",\"futureOwnership\":true}");
            }
            _ => key = "unknown.owner".into(),
        }
        ProjectionStore::put_projection_document(
            f.runtime.client.storage().unwrap(),
            projection_id().unwrap(),
            generation,
            ProjectionDocument::new(key, bytes).unwrap(),
        )
        .await
        .unwrap();
        assert!(f.candidates().await.is_none(), "case {kind}");
        f.collect().await.unwrap();
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
    }
}

#[tokio::test]
async fn reference_admitted_while_collector_waits_is_observed_before_unlink() {
    let f = Fixture::new().await;
    f.file().await;
    f.state(1, true).await;
    let projection = f.runtime.today_projection_lock.lock().await;
    let runtime = f.runtime.clone();
    let artifact = f.receipt.artifact_id();
    let task = tokio::spawn(async move {
        runtime
            .phase1_invalidate_media_artifact(&context(None, 1), artifact)
            .await
    });
    // Synchronize on the first lock's acquisition; no timing assumption about
    // task scheduling or sleeps. The collector cannot acquire the second lock.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while f.runtime.inbound_media_lock.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!task.is_finished());
    f.state(2, true).await;
    drop(projection);
    task.await.unwrap().unwrap();
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
}

#[tokio::test]
async fn interrupted_unlink_retains_ownership_and_retry_only_removes_unreferenced_file() {
    let f = Fixture::new().await;
    f.file().await;
    let artifact = f.receipt.artifact_id();
    let obstructed = f.directory().join(format!("{}.gif", artifact.to_hex()));
    let external = f.root.path().join("external");
    std::fs::write(&external, b"unowned").unwrap();
    std::os::unix::fs::symlink(&external, &obstructed).unwrap();
    assert!(f.collect().await.is_err());
    assert_eq!(std::fs::read(&external).unwrap(), b"unowned");
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
    std::fs::remove_file(obstructed).unwrap();
    f.state(2, true).await;
    f.collect().await.unwrap();
    assert!(f.path().exists());
    f.state(2, false).await;
    f.collect().await.unwrap();
    assert!(!f.path().exists());
}

#[tokio::test]
async fn candidate_limit_retains_excess_and_empty_batch_never_needs_storage() {
    let f = Fixture::new().await;
    let ids: Vec<_> = (0..=MAX_CANDIDATES)
        .map(|i| Phase1MediaArtifactId::from_sha256(BlobHash::digest(&i.to_be_bytes())))
        .collect();
    let result = unreferenced(f.runtime.client.storage().unwrap(), &ids)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.len(), MAX_CANDIDATES);
    assert!(!result.contains(&ids[MAX_CANDIDATES]));
    assert!(
        unreferenced(f.runtime.client.storage().unwrap(), &[])
            .await
            .unwrap()
            .unwrap()
            .is_empty()
    );
}
