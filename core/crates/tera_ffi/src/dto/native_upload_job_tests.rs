//! Synthetic diagnostic regressions; no credential or external request is used.

use super::*;

const HEADER: &str = "synthetic-transient-upload-header";
const DETAILS: &str = "synthetic-private-job-details";

fn synthetic_job() -> FfiNativeUploadJobRecord {
    FfiNativeUploadJobRecord {
        schema_version: UPLOAD_OUTPUT_FFI_SCHEMA_VERSION,
        operation_id: DETAILS.to_owned(),
        draft: FfiDraftStatusRecord {
            coordinate_writable: false,
            coordinate_captured: true,
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft_id: DETAILS.to_owned(),
            revision: 2,
            author_public_key: DETAILS.to_owned(),
            kind: FfiDraftKind::Add,
            command_type: FfiAddCommandType::CreatePhotoUpdate,
            form: None,
            state: FfiOutboxState::MediaUploading,
            card_id: DETAILS.to_owned(),
            operation_id: Some(DETAILS.to_owned()),
            created_at_unix_ms: 1_800_000_000_000,
            updated_at_unix_ms: 1_800_000_000_001,
            media: Vec::new(),
            settlement: None,
            is_revision: false,
            revision_parent_draft_id: None,
        },
        remote_url: format!("https://upload.example/{DETAILS}"),
        upload_url: format!("https://upload.example/{DETAILS}.png"),
        authorization_header: HEADER.to_owned(),
        expected_sha256: DETAILS.to_owned(),
        media_type: "image/png".to_owned(),
        byte_size: 176,
    }
}

fn require_private_fields_omitted(rendered: &str) {
    assert!(
        !rendered.contains(HEADER),
        "transient header must be omitted"
    );
    assert!(
        !rendered.contains(DETAILS),
        "nested job data must be omitted"
    );
    assert!(!rendered.contains("upload.example"), "URLs must be omitted");
    assert!(rendered.len() <= 128, "job diagnostics must remain bounded");
    assert!(rendered.contains("FfiNativeUploadJobRecord"));
}

#[test]
fn native_upload_job_debug_omits_request_fields() {
    require_private_fields_omitted(&format!("{:?}", synthetic_job()));
}

#[test]
fn native_upload_job_pretty_debug_omits_request_fields() {
    require_private_fields_omitted(&format!("{:#?}", synthetic_job()));
}

#[test]
fn nested_native_upload_job_debug_omits_request_fields() {
    let job = synthetic_job();
    require_private_fields_omitted(&format!("{:?}", Some(&job)));
    require_private_fields_omitted(&format!("{:#?}", Ok::<_, ()>(&job)));
}

#[test]
fn native_upload_job_request_fields_remain_exact() {
    let job = synthetic_job();
    let original = job.clone();
    let _ = format!("{job:?}");
    let _ = format!("{job:#?}");
    assert!(job == original);
    assert!(job.authorization_header == HEADER);
    assert!(job.remote_url == format!("https://upload.example/{DETAILS}"));
    assert!(job.upload_url == format!("https://upload.example/{DETAILS}.png"));
    assert_eq!(job.byte_size, 176);
    assert_eq!(job.draft.revision, 2);
}
