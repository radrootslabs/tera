use std::sync::Arc;

use radroots_mobile_ffi::{
    FfiAddCommandType, FfiAddDraftInput, FfiBlossomAuthorityPreference,
    FfiBlossomEndpointAuthority, FfiBlossomHostKind, FfiBlossomPreferencesRecord,
    FfiBlossomUploadIntent, FfiCancellationPolicy, FfiDraftKind, FfiEventTimingKind,
    FfiIdentityCommandKind, FfiIdentityCommandRecord, FfiIdentityLockState, FfiLocalNetworkRecord,
    FfiLocalStoragePolicyRecord, FfiMediaNetworkPolicyRecord, FfiMediaOperation,
    FfiMobileNetworkEnvironment, FfiNativeUploadCompletionInput, FfiOutboxState,
    FfiPreparedMediaInput, FfiProfileMetadataInputRecord, FfiQueuePolicyRecord,
    FfiRelayAccessPreference, FfiRelayAccessRecord, FfiRelayPreferenceRecord,
    FfiRelayPreferencesRecord, FfiRelaySatisfaction, FfiReplaceSettingsRecord,
    FfiRetractionDraftInput, FfiRevisionInputRecord, FfiRevisionPhase, FfiTodayCardType,
    FfiTodayProjectionUpdate, MOBILE_FFI_SCHEMA_VERSION, RadrootsAppError,
};

mod support;

#[tokio::test]
async fn native_boundary_delegates_the_complete_core_surface() {
    let (_root, runtime) = support::runtime().await;
    assert!(runtime.uptime_millis() >= 0);
    assert!(runtime.info_json().contains("sdk"));
    runtime.set_app_info_platform(
        Some("ios".to_owned()),
        Some("org.radroots.app".to_owned()),
        Some("0.1.0-alpha".to_owned()),
        Some("1".to_owned()),
        Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
    );
    assert_eq!(
        runtime.info().app.platform.expect("platform").platform,
        Some("ios".to_owned())
    );
    assert!(!runtime.sdk_capabilities().is_empty());
    assert_eq!(
        runtime.sdk_storage_status().await.expect("storage").backend,
        "sqlite"
    );

    let public = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("default public profile");
    assert_eq!(public.profile, "public");
    assert_eq!(public.state, "configured");
    assert_eq!(public.read_availability, "unavailable");
    assert_eq!(public.write_availability, "unavailable");
    assert_eq!(public.relays.len(), 1);
    assert_eq!(public.relays[0].access, FfiRelayAccessRecord::ReadWrite);
    assert_eq!(public.relays[0].read_state, "unobserved");
    assert_eq!(public.relays[0].write_state, "unobserved");

    runtime
        .configure_public_relays(vec!["wss://write.example".to_owned()])
        .expect("public relays");
    let public = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("public profile");
    assert_eq!(public.relays.len(), 1);
    assert_eq!(public.relays[0].relay_url, "wss://write.example");
    assert_eq!(public.relays[0].access, FfiRelayAccessRecord::ReadWrite);
    assert!(
        runtime
            .configure_public_relays(vec!["ws://127.0.0.1:7447".to_owned()])
            .is_err()
    );

    let settings = runtime.phase1_settings().await.expect("settings");
    runtime
        .phase1_replace_settings(FfiReplaceSettingsRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            expected_revision: settings.revision,
            relays: FfiRelayPreferencesRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                environment: FfiMobileNetworkEnvironment::Public,
                endpoints: vec![
                    FfiRelayPreferenceRecord {
                        schema_version: MOBILE_FFI_SCHEMA_VERSION,
                        url: "wss://read.example".to_owned(),
                        access: FfiRelayAccessPreference::ReadOnly,
                    },
                    FfiRelayPreferenceRecord {
                        schema_version: MOBILE_FFI_SCHEMA_VERSION,
                        url: "wss://write.example".to_owned(),
                        access: FfiRelayAccessPreference::ReadWrite,
                    },
                ],
            },
            blossom: FfiBlossomPreferencesRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                environment: FfiMobileNetworkEnvironment::Public,
                authority: FfiBlossomAuthorityPreference::PublicWebPki,
                primary_origin: "https://media.example".to_owned(),
                fallback_origins: vec![],
            },
            media_network: FfiMediaNetworkPolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                allow_cellular_downloads: true,
                allow_cellular_uploads: true,
                allow_background_transfers: true,
            },
            local_storage: FfiLocalStoragePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                media_cache_bytes: 256 * 1024 * 1024,
                media_cache_artifacts: 1024,
            },
        })
        .await
        .expect("replace settings");
    runtime
        .phase1_apply_settings_to_runtime()
        .await
        .expect("apply settings");
    let applied = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("applied settings profile");
    assert_eq!(applied.relays.len(), 2);
    assert_eq!(applied.relays[0].access, FfiRelayAccessRecord::ReadOnly);
    assert_eq!(applied.relays[1].access, FfiRelayAccessRecord::ReadWrite);

    runtime
        .configure_simulator_relays(vec!["ws://127.0.0.1:7447".to_owned()])
        .expect("simulator relays");
    let simulator = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("simulator profile");
    assert_eq!(simulator.profile, "simulator_local");
    assert_eq!(simulator.relays.len(), 1);
    assert_eq!(simulator.relays[0].access, FfiRelayAccessRecord::ReadWrite);
    assert!(
        runtime
            .phase1_local_network(FfiLocalNetworkRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                id: "simulator".to_owned(),
                label: "Simulator".to_owned(),
                relay_urls: vec!["ws://127.0.0.1:7447".to_owned()],
                locality: None,
                followed_authors: vec![],
                generation: 1,
            })
            .is_ok()
    );
    assert!(
        runtime
            .configure_simulator_relays(vec!["wss://relay.example".to_owned()])
            .is_err()
    );

    runtime
        .configure_device_relays(vec!["wss://10.0.0.5:7447".to_owned()])
        .expect("device relays");
    let device = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("device profile");
    assert_eq!(device.profile, "device_development");
    assert_eq!(device.relays.len(), 1);
    assert_eq!(device.relays[0].relay_url, "wss://10.0.0.5:7447");
    assert_eq!(device.relays[0].access, FfiRelayAccessRecord::ReadWrite);
    assert!(
        runtime
            .configure_device_relays(vec!["wss://127.0.0.1:7447".to_owned()])
            .is_err()
    );

    runtime
        .configure_blossom(
            FfiBlossomHostKind::PhysicalDevice,
            FfiBlossomEndpointAuthority::PublicWebPki,
            "https://media.example".to_owned(),
            vec!["https://fallback.example".to_owned()],
        )
        .expect("public Blossom");
    let blossom = runtime
        .sdk_blossom_configuration()
        .expect("Blossom configuration")
        .expect("configured Blossom");
    assert_eq!(blossom.host_kind, "physical_device");
    assert_eq!(blossom.endpoint_authority, "public_webpki");
    assert_eq!(blossom.primary_origin, "https://media.example");
    assert_eq!(blossom.fallback_origins, ["https://fallback.example"]);
    assert_eq!(blossom.config_fingerprint.len(), 64);
    let evidence = runtime
        .sdk_blossom_evidence()
        .expect("Blossom evidence")
        .expect("configured evidence");
    assert_eq!(evidence.schema_version, 2);
    assert_eq!(evidence.origin, "https://media.example");
    assert_eq!(evidence.config_fingerprint, blossom.config_fingerprint);
    assert_eq!(evidence.state, "configured_unobserved");
    assert_eq!(evidence.transport_security, "public_webpki");
    assert!(evidence.observed_at_unix_ms.is_none());
    assert!(evidence.error_code.is_none());
    assert!(evidence.server_error_code.is_none());
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Simulator,
            FfiBlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:3100".to_owned(),
            vec![],
        )
        .expect("simulator Blossom");
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Simulator,
            FfiBlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:1".to_owned(),
            vec![],
        )
        .expect("unavailable simulator Blossom");
    assert!(runtime.probe_blossom().await.is_err());
    runtime
        .configure_blossom(
            FfiBlossomHostKind::PhysicalDevice,
            FfiBlossomEndpointAuthority::PrivateNetworkDevelopment,
            "https://10.0.0.5:3100".to_owned(),
            vec![],
        )
        .expect("device Blossom");

    assert_eq!(
        runtime
            .phase1_card_add_parity()
            .into_iter()
            .map(|item| item.card_type)
            .collect::<Vec<_>>(),
        vec![
            FfiTodayCardType::Update,
            FfiTodayCardType::PhotoUpdate,
            FfiTodayCardType::Ask,
            FfiTodayCardType::Event,
            FfiTodayCardType::FoodAvailability,
        ]
    );
    assert_eq!(
        runtime
            .phase1_add_schemas()
            .into_iter()
            .map(|schema| schema.command_type)
            .collect::<Vec<_>>(),
        vec![
            FfiAddCommandType::CreateUpdate,
            FfiAddCommandType::CreatePhotoUpdate,
            FfiAddCommandType::CreateAsk,
            FfiAddCommandType::CreateEvent,
            FfiAddCommandType::CreateFoodAvailability,
        ]
    );
    let parity = runtime.phase1_card_add_parity();
    let schemas = runtime.phase1_add_schemas();
    assert_eq!(parity.len(), 5);
    for (index, item) in parity.iter().enumerate() {
        assert_eq!(item.command_type, schemas[index].command_type);
    }
    assert_eq!(
        schemas[1]
            .fields
            .iter()
            .find(|field| field.id == "media")
            .and_then(|field| field.max_items),
        Some(20)
    );
    assert_eq!(
        schemas[3]
            .fields
            .iter()
            .find(|field| field.id == "media")
            .and_then(|field| field.max_items),
        Some(1)
    );
    let local_network = runtime
        .phase1_local_network(FfiLocalNetworkRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            id: "nearby".to_owned(),
            label: "Near me".to_owned(),
            relay_urls: vec!["wss://relay.example".to_owned()],
            locality: Some("u10h".to_owned()),
            followed_authors: vec!["a".repeat(64)],
            generation: 1,
        })
        .expect("valid local network");
    assert_eq!(local_network.id, "nearby");
    let refresh = runtime
        .phase1_refresh_today(
            local_network.clone(),
            1_800_000_001,
            FfiTodayProjectionUpdate::Incremental,
        )
        .await
        .expect("empty Today refresh");
    assert_eq!(refresh.update, FfiTodayProjectionUpdate::Incremental);
    assert_eq!(refresh.source_events, 0);
    assert_eq!(refresh.visible_cards, 0);
    assert!(refresh.content_generation > 0);
    let today = runtime
        .phase1_today_page(local_network.clone(), 20, Some(1_800_000_001), None)
        .await
        .expect("empty Today page");
    assert!(today.items.is_empty());
    assert!(today.next_cursor.is_none());
    assert!(
        runtime
            .phase1_today_page(local_network.clone(), 20, None, None)
            .await
            .is_err()
    );
    assert!(
        runtime
            .phase1_today_page(
                local_network.clone(),
                20,
                Some(1_800_000_001),
                Some("opaque".to_owned()),
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .phase1_search(
                FfiLocalNetworkRecord {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    id: "nearby".to_owned(),
                    label: "Near me".to_owned(),
                    relay_urls: vec!["wss://relay.example".to_owned()],
                    locality: Some("u10h".to_owned()),
                    followed_authors: vec!["a".repeat(64)],
                    generation: 1,
                },
                "carrots".to_owned(),
                20,
                1_800_000_001,
            )
            .await
            .expect("empty search")
            .is_empty()
    );
    let me = runtime
        .phase1_me(
            FfiLocalNetworkRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                id: "nearby".to_owned(),
                label: "Near me".to_owned(),
                relay_urls: vec!["wss://relay.example".to_owned()],
                locality: Some("u10h".to_owned()),
                followed_authors: vec!["a".repeat(64)],
                generation: 1,
            },
            1_800_000_001,
        )
        .await
        .expect("Me snapshot");
    assert_eq!(me.public_key, support::PUBLIC_KEY);

    let add = FfiAddDraftInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        command_type: FfiAddCommandType::CreateUpdate,
        content: "Farm stand opens at noon".to_owned(),
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
        media: Vec::new(),
    };
    runtime
        .phase1_validate_add_draft(add.clone(), 1_800_000_001)
        .expect("valid draft");
    let saved = runtime
        .phase1_save_add_intent(add, None, None)
        .await
        .expect("saved draft");
    let draft_id = saved.draft_id.clone();
    assert_eq!(draft_id.len(), 32);
    assert_eq!(saved.state, FfiOutboxState::Draft);
    assert_eq!(saved.kind, FfiDraftKind::Add);
    assert_eq!(
        saved.form.as_ref().map(|form| form.content.as_str()),
        Some("Farm stand opens at noon")
    );
    assert_eq!(
        runtime
            .phase1_draft_status(draft_id.clone())
            .await
            .expect("draft status")
            .revision,
        saved.revision
    );
    assert_eq!(
        runtime
            .phase1_draft_heads(10)
            .await
            .expect("draft heads")
            .len(),
        1
    );
    let queued = runtime
        .phase1_queue_add_intent(draft_id.clone(), saved.revision)
        .await
        .expect("queued draft");
    assert_eq!(queued.state, FfiOutboxState::Queued);
    let recovered = runtime
        .phase1_recover_add_intent(draft_id.clone())
        .await
        .expect("recovered queue");
    assert_eq!(recovered.revision, queued.revision);
    let cancelled = runtime
        .phase1_cancel_add_intent(draft_id.clone(), recovered.revision)
        .await
        .expect("cancelled draft");
    assert_eq!(cancelled.state, FfiOutboxState::Cancelled);
    assert!(
        runtime
            .phase1_save_add_intent(
                FfiAddDraftInput {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    command_type: FfiAddCommandType::CreateEvent,
                    content: "Market opens Saturday".to_owned(),
                    identifier: None,
                    title: Some("Saturday market".to_owned()),
                    summary: Some("Weekly market".to_owned()),
                    location: Some("Town square".to_owned()),
                    event_timing: Some(FfiEventTimingKind::AllDay),
                    event_start_date: Some("2027-01-02".to_owned()),
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
                    media: Vec::new(),
                },
                None,
                Some(1),
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .phase1_advance_draft(draft_id.clone(), cancelled.revision)
            .await
            .is_err()
    );

    let retraction_id = "0a".repeat(16);
    let retraction_input = FfiRetractionDraftInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        command_type: FfiAddCommandType::CreateUpdate,
        target_card_id: "c".repeat(64),
        target_event_id: "a".repeat(64),
        target_kind: 1,
        target_address: None,
        reason: "Replaced with a corrected copy".to_owned(),
    };
    let mut unsupported_retraction = retraction_input.clone();
    unsupported_retraction.schema_version += 1;
    assert_eq!(
        runtime
            .phase1_save_retraction_draft(
                retraction_id.clone(),
                unsupported_retraction,
                1_800_000_005,
                1_800_000_005_000,
            )
            .await
            .expect_err("unsupported retraction schema")
            .report()
            .code,
        "unsupported_schema_version"
    );
    let retraction = runtime
        .phase1_save_retraction_draft(
            retraction_id.clone(),
            retraction_input,
            1_800_000_005,
            1_800_000_005_000,
        )
        .await
        .expect("saved retraction");
    assert_eq!(retraction.kind, FfiDraftKind::Retraction);
    assert_eq!(retraction.card_id, "c".repeat(64));
    assert!(retraction.form.is_none());
    let queued_retraction = runtime
        .phase1_queue_draft(
            retraction_id.clone(),
            retraction.revision,
            FfiQueuePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                relay_urls: vec!["wss://write.example".to_owned()],
                satisfaction: FfiRelaySatisfaction::AllAccepted,
                delivery_deadline_unix_ms: 1_800_100_000_000,
                cancellation: FfiCancellationPolicy::LocalCooperative,
            },
            1_800_000_006_000,
        )
        .await
        .expect("queued retraction");
    let cancelled_retraction = runtime
        .phase1_cancel_draft(retraction_id, queued_retraction.revision, 1_800_000_007_000)
        .await
        .expect("cancelled retraction");
    assert_eq!(cancelled_retraction.state, FfiOutboxState::Cancelled);

    let upload = FfiBlossomUploadIntent {
        schema_version: MOBILE_FFI_SCHEMA_VERSION + 1,
        draft_id,
        expected_revision: cancelled.revision,
        media: FfiPreparedMediaInput {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            opaque_reference: "media:unused".to_owned(),
            file_descriptor: 0,
            sha256: "00".repeat(32),
            media_type: "image/png".to_owned(),
            byte_size: 1,
            width: 1,
            height: 1,
            alt: "unused".to_owned(),
            prepared_at_unix_s: 1_800_000_000,
        },
    };
    let upload_error = runtime
        .phase1_upload_add_media_intent(upload.clone())
        .await
        .expect_err("unsupported upload schema");
    assert_eq!(upload_error.report().code, "unsupported_schema_version");
    assert_eq!(
        runtime
            .phase1_prepare_add_media_background(upload.clone())
            .await
            .expect_err("unsupported background upload schema")
            .report()
            .code,
        "unsupported_schema_version"
    );
    assert_eq!(
        runtime
            .phase1_complete_add_media_background(FfiNativeUploadCompletionInput {
                schema_version: MOBILE_FFI_SCHEMA_VERSION + 1,
                draft_id: upload.draft_id.clone(),
                expected_revision: upload.expected_revision,
                media: upload.media.clone(),
                status_code: 200,
                response_media_type: Some("application/json".to_owned()),
                response_content_encoding: None,
                response_body: Vec::new(),
            })
            .await
            .expect_err("unsupported completion schema")
            .report()
            .code,
        "invalid_native_upload_completion"
    );
    assert_eq!(
        runtime
            .phase1_complete_add_media_background(FfiNativeUploadCompletionInput {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                draft_id: upload.draft_id.clone(),
                expected_revision: upload.expected_revision,
                media: upload.media.clone(),
                status_code: 200,
                response_media_type: Some("application/json".to_owned()),
                response_content_encoding: None,
                response_body: vec![0; 16_385],
            })
            .await
            .expect_err("oversized completion body")
            .report()
            .code,
        "invalid_native_upload_completion"
    );
    let mut invalid_id_upload = upload;
    invalid_id_upload.schema_version = MOBILE_FFI_SCHEMA_VERSION;
    invalid_id_upload.draft_id = "not-a-draft-id".to_owned();
    assert_eq!(
        runtime
            .phase1_upload_add_media_intent(invalid_id_upload.clone())
            .await
            .expect_err("invalid draft id")
            .report()
            .code,
        "invalid_draft_id"
    );
    assert_eq!(
        runtime
            .phase1_prepare_add_media_background(invalid_id_upload.clone())
            .await
            .expect_err("invalid background draft id")
            .report()
            .code,
        "invalid_draft_id"
    );
    assert_eq!(
        runtime
            .phase1_complete_add_media_background(FfiNativeUploadCompletionInput {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                draft_id: invalid_id_upload.draft_id,
                expected_revision: invalid_id_upload.expected_revision,
                media: invalid_id_upload.media,
                status_code: 200,
                response_media_type: Some("application/json".to_owned()),
                response_content_encoding: None,
                response_body: Vec::new(),
            })
            .await
            .expect_err("invalid completion draft id")
            .report()
            .code,
        "invalid_draft_id"
    );
    assert!(
        runtime
            .phase1_local_network(FfiLocalNetworkRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                id: "nearby".to_owned(),
                label: "Near me".to_owned(),
                relay_urls: Vec::new(),
                locality: None,
                followed_authors: Vec::new(),
                generation: 1,
            })
            .is_err()
    );

    let initial_settings = runtime.phase1_settings().await.expect("settings");
    let begun = runtime
        .phase1_apply_identity_command(
            initial_settings.revision,
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::BeginImport,
                operation_id: Some("import-ffi-1".to_owned()),
                identity_id: None,
                public_key: None,
            },
        )
        .await
        .expect("begin identity import");
    let completed = runtime
        .phase1_apply_identity_command(
            begun.settings.revision,
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::CompleteImport,
                operation_id: Some("import-ffi-1".to_owned()),
                identity_id: Some("primary".to_owned()),
                public_key: Some(support::PUBLIC_KEY.to_owned()),
            },
        )
        .await
        .expect("complete identity import");
    let unlocked = runtime
        .phase1_apply_identity_command(
            completed.settings.revision,
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Unlock,
                operation_id: None,
                identity_id: None,
                public_key: None,
            },
        )
        .await
        .expect("record process-local unlock");
    assert_eq!(
        unlocked.settings.identity.lock_state,
        FfiIdentityLockState::Unlocked
    );
    assert_eq!(unlocked.settings.revision, completed.settings.revision);

    let source_event_id = "ab".repeat(32);
    let source = radroots_mobile_core::runtime::product_surface::CardSourceIdentity::Event(
        radroots_event::EventId::parse(&source_event_id).expect("source event id"),
    );
    let card_id = radroots_mobile_core::runtime::product_surface::CardId::derive(
        radroots_mobile_core::runtime::product_surface::TodayCardType::Update,
        &source,
    )
    .to_hex();
    let revision = runtime
        .phase1_save_revision_intent(FfiRevisionInputRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            card_id,
            source_event_id,
            source_address: None,
            author_public_key: support::PUBLIC_KEY.to_owned(),
            replacement: FfiAddDraftInput {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                command_type: FfiAddCommandType::CreateUpdate,
                content: "Corrected farm stand hours".to_owned(),
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
                media: Vec::new(),
            },
        })
        .await
        .expect("save lossless revision intent");
    assert_eq!(revision.phase, FfiRevisionPhase::ReplacementPending);
    assert_eq!(revision.operation_id, revision.replacement.draft_id);
    assert!(revision.replacement.is_revision);
    assert_eq!(
        runtime
            .phase1_revision_status(revision.operation_id.clone())
            .await
            .expect("revision status")
            .phase,
        FfiRevisionPhase::ReplacementPending
    );
    assert!(
        runtime
            .phase1_advance_revision(revision.operation_id.clone())
            .await
            .is_err()
    );
    let cancelled_revision = runtime
        .phase1_cancel_revision(revision.operation_id.clone())
        .await
        .expect("cancel revision intent");
    assert_eq!(cancelled_revision.operation_id, revision.operation_id);
    assert_eq!(cancelled_revision.phase, FfiRevisionPhase::Cancelled);

    let profile = runtime
        .phase1_save_profile_metadata(FfiProfileMetadataInputRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            name: "grower".to_owned(),
            display_name: Some("Local Grower".to_owned()),
            about: Some("Seasonal produce".to_owned()),
            picture: None,
            banner: None,
            nip05: Some("grower@farm.example".to_owned()),
            bot: Some(false),
        })
        .await
        .expect("save profile intent");
    assert_eq!(profile.state, FfiOutboxState::Draft);
    assert_eq!(profile.operation_id.len(), 32);
    assert_eq!(
        runtime
            .phase1_profile_status(profile.operation_id.clone())
            .await
            .expect("profile status")
            .revision,
        profile.revision
    );
    let cancelled_profile = runtime
        .phase1_cancel_profile(profile.operation_id.clone(), profile.revision)
        .await
        .expect("cancel profile intent");
    assert_eq!(cancelled_profile.operation_id, profile.operation_id);
    assert_eq!(cancelled_profile.state, FfiOutboxState::Cancelled);
    assert_eq!(
        runtime
            .phase1_advance_profile(profile.operation_id.clone())
            .await
            .expect("advance cancelled profile")
            .state,
        FfiOutboxState::Cancelled
    );

    let cache = runtime
        .phase1_media_cache_status(local_network.clone())
        .await
        .expect("empty media cache status");
    assert_eq!(cache.artifact_count, 0);
    assert_eq!(cache.total_bytes, 0);
    assert!(cache.configuration_fingerprint.is_none());
    assert!(
        runtime
            .phase1_verified_media_artifact(local_network.clone(), "11".repeat(32))
            .await
            .expect("empty verified media lookup")
            .is_none()
    );
    assert!(
        !runtime
            .phase1_invalidate_media_artifact(local_network.clone(), "11".repeat(32))
            .await
            .expect("missing media invalidation")
    );
    assert!(
        runtime
            .phase1_invalidate_media_configuration(local_network.clone(), "22".repeat(32))
            .await
            .expect("empty configuration invalidation")
            .is_empty()
    );
    let media_operation = Arc::new(FfiMediaOperation::new().expect("media operation"));
    let media_error = runtime
        .phase1_retrieve_media(
            local_network.clone(),
            "invalid-reference".to_owned(),
            Arc::clone(&media_operation),
        )
        .await
        .expect_err("invalid media reference");
    assert_eq!(
        media_error.report().code,
        "invalid_media_reference_fingerprint"
    );
    assert_eq!(
        runtime
            .phase1_retrieve_media(local_network, "00".repeat(32), Arc::clone(&media_operation),)
            .await
            .expect_err("single-use media operation")
            .report()
            .code,
        "media_operation_already_used"
    );

    runtime.shutdown().await.expect("shutdown");
    assert!(matches!(
        runtime.sdk_storage_status().await,
        Err(RadrootsAppError::Failure { .. })
    ));
    assert!(matches!(
        runtime.sdk_relay_status(),
        Err(RadrootsAppError::Failure { .. })
    ));
    assert!(matches!(
        runtime.configure_public_relays(Vec::new()),
        Err(RadrootsAppError::Failure { .. })
    ));
}
