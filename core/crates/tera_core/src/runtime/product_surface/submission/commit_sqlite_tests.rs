use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    lifecycle::RuntimeLifecycleError,
    product_surface::{
        AddCommandType, ComposerEditSequence, ComposerPartialForm, Phase1DraftError,
        submission::test_support::*,
    },
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_sdk::transport::{
    RelayAccess, RelayEndpoint, RelayProfile, RelayProfileKind, RelayUrlPolicy,
};

async fn runtime(root: &std::path::Path, author: &str) -> TeraRuntime {
    let config = MobileUserStoreConfig::from_encoded(
        root,
        author,
        &hex::encode([1; 32]),
        NOW,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    RuntimeBuilder::new(config)
        .relay_profile(
            RelayProfile::explicit(
                RelayProfileKind::Simulator,
                [RelayEndpoint::new(
                    "ws://127.0.0.1:19999",
                    RelayUrlPolicy::Local,
                    RelayAccess::ReadWrite,
                )
                .unwrap()],
            )
            .unwrap(),
        )
        .blossom_config(radroots_sdk::transport::BlossomConfig::from_profile(
            blossom().profile().unwrap(),
        ))
        .build()
        .await
        .unwrap()
}

#[tokio::test]
async fn actual_sqlite_concurrent_prepare_replays_after_edits_settings_and_reconstruction() {
    for media in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = runtime(root.path(), AUTHOR).await;
        let request = request();
        let mut input = input(if media {
            AddCommandType::CreatePhotoUpdate
        } else {
            AddCommandType::CreateUpdate
        });
        let bytes = if media {
            let (photo, bytes) = photo();
            input.media.push(photo);
            vec![bytes]
        } else {
            vec![]
        };
        let form = ComposerPartialForm::new(input).unwrap();
        runtime
            .composer_create(
                request.scope(),
                request.composer_id(),
                ComposerEditSequence::INITIAL,
                form.clone(),
            )
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            runtime.submission_prepare(&request, bytes.clone()),
            runtime.submission_prepare(&request, bytes)
        );
        let a = a.unwrap();
        let b = b.unwrap();
        assert_eq!(a.operation_id(), b.operation_id());
        assert_eq!(a.intent_id(), b.intent_id());
        assert_ne!(a.is_replay(), b.is_replay());
        let later = runtime
            .composer_save(
                request.scope(),
                request.composer_id(),
                request.expected_revision(),
                ComposerEditSequence::new(2).unwrap(),
                ComposerPartialForm::new(super::super::test_support::input(
                    AddCommandType::CreateAsk,
                ))
                .unwrap(),
            )
            .await
            .unwrap();
        // A read-only profile makes a new capture impossible, but cannot invalidate the receipt.
        runtime
            .client
            .configure_nostr(
                RelayProfile::explicit(
                    RelayProfileKind::Simulator,
                    [RelayEndpoint::new(
                        "ws://127.0.0.1:19998",
                        RelayUrlPolicy::Local,
                        RelayAccess::ReadOnly,
                    )
                    .unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
        assert!(
            runtime
                .active_queue_policy(a.captured_at_unix_ms())
                .is_err()
        );
        let replay = runtime.submission_prepare(&request, vec![]).await.unwrap();
        assert!(replay.is_replay());
        assert_eq!(replay.operation_id(), a.operation_id());
        assert_eq!(
            runtime
                .composer_list(request.scope(), 10, None)
                .await
                .unwrap()
                .entries()
                .len(),
            1
        );
        assert!(runtime.phase1_draft_heads(20).await.unwrap().is_empty());
        assert_eq!(
            runtime
                .phase1_draft_status(*a.intent_id().as_bytes())
                .await
                .unwrap_err(),
            Phase1DraftError::Corrupt
        );
        assert_eq!(
            runtime
                .phase1_queue_draft(
                    *a.intent_id().as_bytes(),
                    1,
                    policy("wss://relay.example"),
                    NOW
                )
                .await
                .unwrap_err(),
            Phase1DraftError::Corrupt
        );
        assert_eq!(
            runtime
                .phase1_cancel_draft(*a.intent_id().as_bytes(), 1, NOW)
                .await
                .unwrap_err(),
            Phase1DraftError::Corrupt
        );
        runtime.shutdown().await.unwrap();
        assert_eq!(
            runtime.submission_recover(&request).await.unwrap_err(),
            E::Reservation(SubmissionReservationError::Lifecycle(
                RuntimeLifecycleError::Closed
            ))
        );
        drop(runtime);
        let runtime = self::runtime(root.path(), AUTHOR).await;
        // Simulates a caller lost after commit: no previous native handle and no materialized media.
        let replay = runtime.submission_prepare(&request, vec![]).await.unwrap();
        assert!(replay.is_replay());
        assert_eq!(replay.operation_id(), a.operation_id());
        assert_eq!(
            runtime
                .composer_load(request.scope(), request.composer_id())
                .await
                .unwrap(),
            *later.draft()
        );
        let mut foreign = request.clone();
        foreign.scope = scope(OTHER, "nearby");
        assert_eq!(
            runtime.submission_recover(&foreign).await.unwrap_err(),
            E::Reservation(SubmissionReservationError::Source(
                ComposerPersistenceError::ScopeMismatch
            ))
        );
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let runtime = self::runtime(root.path(), OTHER).await;
        assert!(
            runtime
                .submission_recover(&foreign)
                .await
                .unwrap()
                .is_none()
        );
        runtime
            .composer_create(
                foreign.scope(),
                foreign.composer_id(),
                ComposerEditSequence::INITIAL,
                form,
            )
            .await
            .unwrap();
        let bytes = if media { vec![photo().1] } else { vec![] };
        let separate = runtime.submission_prepare(&foreign, bytes).await.unwrap();
        assert_ne!(a.operation_id(), separate.operation_id());
        assert_ne!(a.intent_id(), separate.intent_id());
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn actual_sqlite_new_commit_loses_to_composer_edit_without_any_submission_record() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path(), AUTHOR).await;
    let request = request();
    runtime
        .composer_create(
            request.scope(),
            request.composer_id(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(input(AddCommandType::CreateUpdate)).unwrap(),
        )
        .await
        .unwrap();
    let captured = runtime.submission_capture(&request, vec![]).await.unwrap();
    runtime
        .composer_save(
            request.scope(),
            request.composer_id(),
            request.expected_revision(),
            ComposerEditSequence::new(2).unwrap(),
            ComposerPartialForm::new(input(AddCommandType::CreateAsk)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        runtime.submission_commit(&captured).await.unwrap_err(),
        E::RevisionConflict
    );
    assert!(
        runtime
            .submission_recover(&request)
            .await
            .unwrap()
            .is_none()
    );
    let store = runtime.client.storage().unwrap();
    assert!(
        store
            .authored_operation(intent::operation_id(&request).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .authored_draft_head(intent::intent_id(&request).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .authored_draft_heads(*request.scope().author().as_bytes(), 20)
            .await
            .unwrap()
            .len(),
        2
    );
    runtime.shutdown().await.unwrap();
}
