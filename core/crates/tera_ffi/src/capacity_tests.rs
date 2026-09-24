use super::*;
use tera_core::runtime::product_surface::{
    ComposerPersistenceError, SubmissionCommitError, SubmissionReservationError,
};

#[test]
fn capacity_errors_never_claim_retry_or_rollback_and_preserve_original_recovery() {
    let errors: Vec<TeraAppError> = vec![
        ComposerPersistenceError::Storage(radroots_storage::Error::SpaceInsufficient).into(),
        SubmissionCommitError::Storage(radroots_storage::Error::SpaceInsufficient).into(),
        SubmissionReservationError::Storage(radroots_storage::Error::SpaceInsufficient).into(),
        TodayError::Storage(radroots_storage::Error::SpaceInsufficient).into(),
        TodayError::InboundMedia(Phase1InboundMediaError::SpaceInsufficient).into(),
        Phase1DraftError::SpaceInsufficient.into(),
        SettingsError::SpaceInsufficient.into(),
    ];
    for error in errors {
        let TeraAppError::Failure { report } = error;
        assert_eq!(report.code, "storage_space_insufficient");
        assert!(!report.retryable);
        assert!(
            report
                .recovery_actions
                .iter()
                .any(|action| action == "reconcile_original_operation")
        );
        assert!(
            report
                .recovery_actions
                .iter()
                .any(|action| action == "preserve_local_work")
        );
    }
}
