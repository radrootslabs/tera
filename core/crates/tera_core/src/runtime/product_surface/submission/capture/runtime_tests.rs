use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};

#[tokio::test]
async fn durable_source_survives_strict_failure_edits_restart_and_account_switch() {
    let root = tempfile::tempdir().unwrap();
    let build = |author: &str, generation: u8| {
        let config = MobileUserStoreConfig::from_encoded(
            root.path(),
            author,
            &hex::encode([generation; 32]),
            NOW,
            ProtectedDataAvailability::Available,
        )
        .unwrap();
        std::fs::create_dir_all(config.owner_directory()).unwrap();
        RuntimeBuilder::new(config).build()
    };
    let runtime = build(AUTHOR, 1).await.unwrap();
    let request = request();
    let blank =
        ComposerPartialForm::new(ComposerFormInput::empty(AddCommandType::CreateUpdate)).unwrap();
    runtime
        .composer_create(
            request.scope(),
            request.composer_id(),
            ComposerEditSequence::INITIAL,
            blank,
        )
        .await
        .unwrap();
    let before = runtime
        .composer_load(request.scope(), request.composer_id())
        .await
        .unwrap();
    assert!(matches!(
        runtime.submission_capture(&request, vec![]).await,
        Err(SubmissionCaptureError::InvalidInput("invalid_update"))
    ));
    assert_eq!(
        runtime
            .composer_load(request.scope(), request.composer_id())
            .await
            .unwrap(),
        before
    );
    let saved = runtime
        .composer_save(
            request.scope(),
            request.composer_id(),
            request.expected_revision(),
            ComposerEditSequence::new(2).unwrap(),
            ComposerPartialForm::new(input(AddCommandType::CreateUpdate)).unwrap(),
        )
        .await
        .unwrap();
    let mut request = SubmissionReservationRequest::new(
        super::super::super::SubmissionCommandId::generate().unwrap(),
        request.scope().clone(),
        request.composer_id(),
        saved.draft().revision(),
    );
    let captured = runtime.submission_capture(&request, vec![]).await.unwrap();
    runtime
        .composer_save(
            request.scope(),
            request.composer_id(),
            request.expected_revision(),
            ComposerEditSequence::new(3).unwrap(),
            ComposerPartialForm::new(input(AddCommandType::CreateAsk)).unwrap(),
        )
        .await
        .unwrap();
    let replay = runtime.submission_capture(&request, vec![]).await.unwrap();
    assert!(captured.same_request(&replay));
    runtime.shutdown().await.unwrap();
    drop(runtime);
    // A new host session retains the durable store generation.
    let runtime = build(AUTHOR, 1).await.unwrap();
    assert!(captured.same_request(&runtime.submission_capture(&request, vec![]).await.unwrap()));
    request.scope = scope(OTHER, "nearby");
    assert!(matches!(
        runtime.submission_capture(&request, vec![]).await,
        Err(SubmissionCaptureError::Reservation(
            SubmissionReservationError::Source(ComposerPersistenceError::ScopeMismatch)
        ))
    ));
    assert_eq!(captured.plan().author().to_string(), AUTHOR);
    runtime.shutdown().await.unwrap();
}
