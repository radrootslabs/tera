use std::{sync::atomic::Ordering, time::Duration};

use radroots_blossom::authorization::{
    AuthorizationClaim, AuthorizationTarget, AuthorizationValidation, ServerDomain,
    ServerScopeRequirement,
};
use radroots_nostr::blossom::decode_verify_authorization_header;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{media_test_support::*, operation_test_support::*, test_support::*};
use crate::runtime::product_surface::Phase1MediaStage;

#[tokio::test]
async fn authorization_is_associated_before_signer_and_survives_interruption_and_reopen() {
    for foreground in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let original = runtime.submission_operation_status(&request).await.unwrap();
        signer.pause.store(true, Ordering::SeqCst);
        let task = {
            let runtime = runtime.clone();
            let input = upload(&request, 1);
            tokio::spawn(async move {
                if foreground {
                    runtime.submission_upload_media(input).await.map(|_| ())
                } else {
                    runtime
                        .submission_prepare_native_upload(input)
                        .await
                        .map(|_| ())
                }
            })
        };
        tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
            .await
            .unwrap();
        let reserved = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(reserved.media()[0].stage(), Phase1MediaStage::Uploading);
        assert_eq!(reserved.captured(), original.captured());
        assert_eq!(reserved.push(), original.push());
        let payload: Value = serde_json::from_slice(reserved.intent().payload()).unwrap();
        let attempt = &payload["media"][0]["authorization_attempt"];
        let signing = signer.requests.lock().unwrap()[0].clone();
        assert_eq!(
            attempt["operation_id"],
            json!(signing.intent_id().operation_id().as_bytes())
        );
        assert_eq!(
            attempt["artifact_id"],
            json!(signing.intent_id().artifact_id().as_bytes())
        );
        assert_eq!(attempt["created_at_unix_s"], signing.created_at());
        assert_eq!(attempt["expiration_unix_s"], signing.created_at() + 300);
        assert_eq!(
            attempt["content_sha256"],
            json!(Sha256::digest(signing.content()).to_vec())
        );
        assert!(!String::from_utf8_lossy(reserved.intent().payload()).contains("Nostr "));
        assert!(signing.authored_plan().is_none());
        assert!(signing.blossom_authorization_plan().is_some());
        assert_redacted(&reserved);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened =
            self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        assert_eq!(
            reopened
                .submission_operation_status(&request)
                .await
                .unwrap(),
            reserved
        );
        let revision = reserved.intent().revision().get();
        assert!(
            reopened
                .submission_prepare_native_upload(upload(&request, revision))
                .await
                .is_err()
        );
        assert!(
            reopened
                .submission_upload_media(upload(&request, revision))
                .await
                .is_err()
        );
        assert_eq!(signer.count(), 1);
        assert_eq!(
            reopened
                .submission_operation_status(&request)
                .await
                .unwrap(),
            reserved
        );
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn actual_http_authority_is_exact_bounded_and_cannot_be_extended_or_reissued() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (reserved, job) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    let signed = signer.requests.lock().unwrap()[0].clone();
    let sha = radroots_blossom::Sha256::digest(&photo().1);
    let validation = |target, server: &str, now| {
        AuthorizationValidation::new(
            target,
            ServerDomain::parse(server).unwrap(),
            ServerScopeRequirement::RequiredAnyMatch,
            now,
            300,
        )
        .unwrap()
    };
    let valid = validation(
        AuthorizationTarget::Upload(sha),
        "127.0.0.1",
        signed.created_at() + 5,
    );
    assert!(decode_verify_authorization_header(job.authorization_header(), &valid).is_ok());
    for invalid in [
        validation(
            AuthorizationTarget::Upload(radroots_blossom::Sha256::digest(b"other")),
            "127.0.0.1",
            signed.created_at() + 5,
        ),
        validation(
            AuthorizationTarget::DeleteBlob(sha),
            "127.0.0.1",
            signed.created_at() + 5,
        ),
        validation(
            AuthorizationTarget::Upload(sha),
            "other.example",
            signed.created_at() + 5,
        ),
        validation(
            AuthorizationTarget::Upload(sha),
            "127.0.0.1",
            signed.created_at() + 300,
        ),
    ] {
        assert!(decode_verify_authorization_header(job.authorization_header(), &invalid).is_err());
    }
    for excessive_lifetime in [false, true] {
        let mut tags = signed.tags().to_vec();
        if excessive_lifetime {
            tags.iter_mut().find(|tag| tag[0] == "expiration").unwrap()[1] =
                (signed.created_at() + 301).to_string();
        } else {
            tags.push(tags.iter().find(|tag| tag[0] == "t").unwrap().clone());
        }
        assert!(
            AuthorizationClaim::parse(signed.content(), signed.created_at(), &tags)
                .and_then(|claim| claim.validate(&valid))
                .is_err()
        );
    }
    assert_eq!(
        job.operation_id(),
        *signed.intent_id().operation_id().as_bytes()
    );
    assert!(
        runtime
            .submission_prepare_native_upload(upload(&request, reserved.intent().revision().get()))
            .await
            .is_err()
    );
    assert_eq!(signer.count(), 1);
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        reserved
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn historical_metadata_roundtrips_and_attempt_versions_fail_closed() {
    use crate::runtime::product_surface::Phase1MediaPrerequisite;
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer, "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let original = runtime.submission_operation_status(&request).await.unwrap();
    let pending = serde_json::to_value(&original.media()[0]).unwrap();
    assert!(pending.get("authorization_attempt").is_none());
    let historical: Phase1MediaPrerequisite = serde_json::from_value(pending.clone()).unwrap();
    historical.validate().unwrap();
    assert_eq!(serde_json::to_value(historical).unwrap(), pending);
    let (reserved, _) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    let current = serde_json::to_value(&reserved.media()[0]).unwrap();
    for (field, invalid) in [
        ("schema_version", json!(2)),
        ("operation_id", json!(vec![0; 16])),
        ("artifact_id", json!(vec![0; 16])),
        ("content_sha256", json!(vec![0; 32])),
        ("expiration_unix_s", json!(0)),
        ("signing_deadline_unix_ms", json!(0)),
    ] {
        let mut corrupt = current.clone();
        corrupt["authorization_attempt"][field] = invalid;
        let decoded: Phase1MediaPrerequisite = serde_json::from_value(corrupt).unwrap();
        assert!(decoded.validate().is_err(), "{field}");
    }
    let mut unknown = current.clone();
    unknown["authorization_attempt"]["bearer"] = json!("forbidden");
    assert!(serde_json::from_value::<Phase1MediaPrerequisite>(unknown).is_err());
    let mut legacy_uploading = current;
    legacy_uploading
        .as_object_mut()
        .unwrap()
        .remove("authorization_attempt");
    let legacy: Phase1MediaPrerequisite = serde_json::from_value(legacy_uploading).unwrap();
    legacy.validate().unwrap();
    assert_eq!(legacy.stage(), Phase1MediaStage::Uploading);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_upload_paths_persist_before_signing_and_refuse_uncertain_replacement() {
    use crate::runtime::product_surface::{Phase1DraftError, Phase1UploadIntent};
    let input = |revision| {
        let (media, bytes) = photo();
        Phase1UploadIntent::new(
            [84; 16],
            revision,
            bytes,
            radroots_blossom::MediaType::parse(&media.media_type).unwrap(),
            media.width,
            media.height,
        )
        .unwrap()
    };
    for foreground in [false, true] {
        let signer = CountingSigner::new();
        let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let capture = runtime
            .submission_capture(&request, vec![photo().1])
            .await
            .unwrap();
        runtime
            .phase1_save_draft(
                [84; 16],
                capture.command().clone(),
                capture.plan().created_at(),
                capture.media().to_vec(),
                None,
                capture.reservation().reserved_at_unix_ms(),
            )
            .await
            .unwrap();
        assert_eq!(
            runtime
                .phase1_prepare_native_upload(input(99))
                .await
                .err()
                .unwrap(),
            Phase1DraftError::RevisionConflict
        );
        assert_eq!(signer.count(), 0);
        signer.pause.store(true, Ordering::SeqCst);
        let task = {
            let runtime = runtime.clone();
            let input = input(1);
            tokio::spawn(async move {
                if foreground {
                    runtime
                        .phase1_upload_add_media_intent(input)
                        .await
                        .map(|_| ())
                } else {
                    runtime
                        .phase1_prepare_native_upload(input)
                        .await
                        .map(|_| ())
                }
            })
        };
        tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
            .await
            .unwrap();
        let reserved = runtime.phase1_draft_status([84; 16]).await.unwrap();
        assert_eq!(reserved.media()[0].stage(), Phase1MediaStage::Uploading);
        let media = serde_json::to_value(&reserved.media()[0]).unwrap();
        assert_eq!(
            media["authorization_attempt"]["operation_id"],
            json!(
                signer.requests.lock().unwrap()[0]
                    .intent_id()
                    .operation_id()
                    .as_bytes()
            )
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let revision = reserved.draft().revision().get();
        assert_eq!(
            runtime
                .phase1_prepare_native_upload(input(revision))
                .await
                .err()
                .unwrap(),
            Phase1DraftError::InvalidMedia
        );
        assert_eq!(
            runtime
                .phase1_upload_add_media_intent(input(revision))
                .await
                .err()
                .unwrap(),
            Phase1DraftError::InvalidMedia
        );
        assert_eq!(signer.count(), 1);
        assert_eq!(
            runtime.phase1_draft_status([84; 16]).await.unwrap().draft(),
            reserved.draft()
        );
        runtime.shutdown().await.unwrap();
    }
}
