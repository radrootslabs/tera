//! Fixture producer compiled only against the exact historical Lib archive.
//! An ephemeral signer stays in memory; only public expectations are emitted.
use radroots_mobile_ffi::*;
use secp256k1::{Keypair, Message, Secp256k1};
use serde_json::{Value, json};
use std::os::fd::AsRawFd;
use std::path::Path;

struct EphemeralSigner(Keypair);
#[async_trait::async_trait]
impl RadrootsHostSigner for EphemeralSigner {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            availability: SignerAvailabilityRecord::Ready,
        }
    }
    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        assert_eq!(request.public_key, self.0.x_only_public_key().0.to_string());
        let digest: [u8; 32] = request.event_id_digest.try_into().unwrap();
        let signature =
            Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(digest), &self.0);
        HostSigningResult {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            outcome: HostSigningOutcome::Signed,
            operation_id: request.operation_id,
            signer_request_id: request.signer_request_id,
            public_key: request.public_key,
            purpose: request.purpose,
            signature_hex: Some(signature.to_string()),
            completed_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap(),
        }
    }
}

fn input(content: String, media: Vec<FfiPreparedMediaInput>) -> FfiAddDraftInput {
    FfiAddDraftInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        command_type: if media.is_empty() {
            FfiAddCommandType::CreateUpdate
        } else {
            FfiAddCommandType::CreatePhotoUpdate
        },
        content,
        identifier: None,
        title: None,
        summary: None,
        location: None,
        event_timing: None,
        event_start_date: None,
        event_end_date: None,
        event_start_unix_s: None,
        event_end_unix_s: None,
        event_timezone: None,
        price_amount: None,
        currency: None,
        unit: None,
        quantity: None,
        food_published_at_unix_s: None,
        food_status: None,
        media,
    }
}

fn expectation(status: &FfiDraftStatusRecord) -> Value {
    let settled = status.settlement.as_ref().map(|s| {
        json!({
            "signed":s.signed,"admitted":s.admitted,"pending":s.pending,
            "delivery_plans":s.delivery_plans,"delivery_satisfied":s.delivery_satisfied
        })
    });
    json!({ "draft_id": status.draft_id, "revision":status.revision,
        "author_public_key":status.author_public_key,"card_id":status.card_id,
        "operation_id":status.operation_id,"state":format!("{:?}",status.state),
        "created_at_unix_ms":status.created_at_unix_ms,
        "updated_at_unix_ms":status.updated_at_unix_ms,"settlement":settled })
}

async fn queue(
    runtime: &RadrootsRuntime,
    status: &FfiDraftStatusRecord,
    at: u64,
) -> FfiDraftStatusRecord {
    runtime
        .phase1_queue_draft(
            status.draft_id.clone(),
            status.revision,
            FfiQueuePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                relay_urls: vec![
                    "wss://relay-one.example".into(),
                    "wss://relay-two.example".into(),
                ],
                satisfaction: FfiRelaySatisfaction::AllAccepted,
                delivery_deadline_unix_ms: 2_000_000_000_000,
                cancellation: FfiCancellationPolicy::LocalCooperative,
            },
            at,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn produce_pre_refactor_owner_state() {
    if let Ok(root) = std::env::var("TERA_LEGACY_DOWNGRADE_ROOT") {
        refuse_current_owner_downgrade(Path::new(&root)).await;
        return;
    }
    let output = std::env::var("TERA_LEGACY_FIXTURE_OUTPUT").unwrap();
    let root = Path::new(&output);
    let host: Value =
        serde_json::from_slice(&std::fs::read(root.join("host-metadata.json")).unwrap()).unwrap();
    let generation = host["source_generation"].as_str().unwrap();
    let key = Keypair::new(&Secp256k1::new(), &mut secp256k1::rand::thread_rng());
    let transient_secret = key.secret_key().secret_bytes();
    let public_key = key.x_only_public_key().0.to_string();
    let data = root.join("data");
    std::fs::create_dir_all(data.join("radroots/users").join(&public_key)).unwrap();
    let runtime = RadrootsRuntime::with_host_signer(
        data.to_string_lossy().into(),
        public_key.clone(),
        generation.into(),
        host["generation_created_at_unix_ms"].as_u64().unwrap(),
        ProtectedDataAvailability::Available,
        Box::new(EphemeralSigner(key)),
    )
    .await
    .unwrap();
    let preferences = runtime.phase1_settings().await.unwrap();
    runtime
        .phase1_replace_settings(FfiReplaceSettingsRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            expected_revision: preferences.revision,
            relays: FfiRelayPreferencesRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                environment: FfiMobileNetworkEnvironment::Public,
                endpoints: ["wss://relay-one.example", "wss://relay-two.example"]
                    .into_iter()
                    .map(|url| FfiRelayPreferenceRecord {
                        schema_version: MOBILE_FFI_SCHEMA_VERSION,
                        url: url.into(),
                        access: FfiRelayAccessPreference::ReadWrite,
                    })
                    .collect(),
            },
            blossom: FfiBlossomPreferencesRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                environment: FfiMobileNetworkEnvironment::Public,
                authority: FfiBlossomAuthorityPreference::PublicWebPki,
                primary_origin: "https://blossom.example".into(),
                fallback_origins: vec![],
            },
            media_network: preferences.media_network,
            local_storage: preferences.local_storage,
        })
        .await
        .unwrap();
    runtime.phase1_apply_settings_to_runtime().await.unwrap();
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Native,
            FfiBlossomEndpointAuthority::PublicWebPki,
            "https://blossom.example".into(),
            vec![],
        )
        .unwrap();
    let image =
        std::fs::File::open(root.join(host["staged_relative_path"].as_str().unwrap())).unwrap();
    let media = FfiPreparedMediaInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        opaque_reference: format!("media:{}", host["media_sha256"].as_str().unwrap()),
        file_descriptor: image.as_raw_fd().try_into().unwrap(),
        sha256: host["media_sha256"].as_str().unwrap().into(),
        media_type: "image/png".into(),
        byte_size: host["media_bytes"].as_u64().unwrap(),
        width: host["media_width"].as_u64().unwrap().try_into().unwrap(),
        height: host["media_height"].as_u64().unwrap().try_into().unwrap(),
        alt: "Historical synthetic image é".into(),
        prepared_at_unix_s: 1_786_000_000,
    };
    let media_id = format!("{:032x}", 1);
    let saved = runtime
        .phase1_save_draft(
            media_id.clone(),
            input("Historical pending photo é".into(), vec![media.clone()]),
            1_786_000_000,
            None,
            1_786_000_000_000,
        )
        .await
        .unwrap();
    let job = runtime
        .phase1_prepare_add_media_background(FfiBlossomUploadIntent {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft_id: media_id,
            expected_revision: saved.revision,
            media,
        })
        .await
        .unwrap();
    let native = json!({"draft_id":job.draft.draft_id,"revision":job.draft.revision,
        "operation_id":job.operation_id,"remote_url":job.remote_url,
        "sha256":job.expected_sha256,"byte_size":job.byte_size});
    // Never serialize the returned upload authorization header.
    let transient_authorization = job.authorization_header.as_bytes().to_vec();
    assert!(!transient_authorization.is_empty());
    let pending_updated_at = job.draft.updated_at_unix_ms;
    let pending_id = job.draft.draft_id.clone();
    let mut statuses = vec![expectation(&job.draft)];
    drop(job);
    for index in 2..=106 {
        let id = format!("{index:032x}");
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .try_into()
            .unwrap();
        assert!(at >= pending_updated_at);
        let saved = runtime
            .phase1_save_draft(
                id,
                input(
                    format!("Historical synthetic draft {index} e\u{301}"),
                    vec![],
                ),
                1_786_000_000,
                None,
                at,
            )
            .await
            .unwrap();
        let status = if index == 2 || index == 3 {
            let queued = queue(&runtime, &saved, at).await;
            if index == 3 {
                runtime
                    .phase1_sign_queued_draft(queued.draft_id.clone(), queued.revision)
                    .await
                    .unwrap()
            } else {
                queued
            }
        } else {
            saved
        };
        statuses.push(expectation(&status));
    }
    let first_page = runtime.phase1_draft_heads(100).await.unwrap();
    assert_eq!(first_page.len(), 100);
    assert!(!first_page.iter().any(|row| row.draft_id == pending_id));
    runtime.shutdown().await.unwrap();
    drop(runtime);
    drop(image);
    reject_secret(root, &transient_secret);
    reject_authorization(root, &transient_authorization);
    std::fs::write(root.join("rust-metadata.json"),serde_json::to_vec_pretty(&json!({
        "public_key":public_key,"source_generation":generation,"drafts":statuses,
        "native_pending":native,"actual_first_page_count":100,"pending_media_outside_first_page":true,
        "secret_scan":"Actual closed owner files contain neither transient signer bytes nor hexadecimal secret; neither form is emitted.",
        "authorization_scan":"Actual closed owner files contain no returned upload authorization header; its value is never emitted.",
        "network_effects":"No socket or HTTP delivery was requested; only local save, queue, signing/admission and upload-job preparation."})).unwrap()).unwrap();
}

fn reject_authorization(root: &Path, authorization: &[u8]) {
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            reject_authorization(&path, authorization);
        } else {
            let bytes = std::fs::read(path).unwrap();
            assert!(
                !bytes
                    .windows(authorization.len())
                    .any(|part| part == authorization),
                "Transient authorization reached fixture output"
            );
        }
    }
}

fn reject_secret(root: &Path, secret: &[u8; 32]) {
    let encoded = secret
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            reject_secret(&path, secret);
        } else {
            let bytes = std::fs::read(path).unwrap();
            assert!(
                !bytes.windows(32).any(|part| part == secret),
                "Transient signer bytes reached fixture output"
            );
            assert!(
                !bytes
                    .windows(encoded.len())
                    .any(|part| part.eq_ignore_ascii_case(encoded.as_bytes())),
                "Transient signer encoding reached fixture output"
            );
        }
    }
}

async fn refuse_current_owner_downgrade(root: &Path) {
    let managed = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let root = root.canonicalize().unwrap();
    assert!(root.starts_with(managed) && root.file_name().unwrap() == "old_reader_probe");
    let host: Value =
        serde_json::from_slice(&std::fs::read(root.join("host-metadata.json")).unwrap()).unwrap();
    let rust: Value =
        serde_json::from_slice(&std::fs::read(root.join("rust-metadata.json")).unwrap()).unwrap();
    let public = rust["public_key"].as_str().unwrap();
    let owner = root.join("data/radroots/users").join(public);
    let before: [Vec<u8>; 2] =
        ["runtime.sqlite", "private.sqlite"].map(|name| std::fs::read(owner.join(name)).unwrap());
    let result = RadrootsRuntime::new(
        root.join("data").to_string_lossy().into(),
        public.into(),
        host["source_generation"].as_str().unwrap().into(),
        host["generation_created_at_unix_ms"].as_u64().unwrap(),
        ProtectedDataAvailability::Available,
    )
    .await;
    assert!(
        result.is_err(),
        "Historical reader must refuse a newer current-owner schema"
    );
    let failure = result.err().unwrap();
    assert_eq!(failure.report().code, "schema_too_new");
    assert_eq!(
        failure.report().safe_message,
        "SDK persistent storage schema is newer than this runtime"
    );
    for (index, name) in ["runtime.sqlite", "private.sqlite"].iter().enumerate() {
        assert_eq!(std::fs::read(owner.join(name)).unwrap(), before[index]);
    }
}
