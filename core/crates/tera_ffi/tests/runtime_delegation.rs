use radroots_mobile_ffi::{
    FfiAddCommandType, FfiAddDraftInput, FfiBlossomUploadInput, FfiCancellationPolicy,
    FfiLocalNetworkRecord, FfiOutboxState, FfiPreparedMediaInput, FfiQueuePolicyRecord,
    FfiRelaySatisfaction, FfiTodayCardType, FfiTodayProjectionUpdate, MOBILE_FFI_SCHEMA_VERSION,
    RadrootsAppError,
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
    assert_eq!(public.relays[0].access, "read_only");
    assert_eq!(public.relays[0].read_state, "unobserved");
    assert_eq!(public.relays[0].write_state, "unsupported");

    runtime
        .configure_public_relays(vec!["wss://write.example".to_owned()])
        .expect("public relays");
    let public = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("public profile");
    assert_eq!(public.relays.len(), 2);
    assert_eq!(public.relays[1].access, "read_write");
    assert!(
        runtime
            .configure_public_relays(vec!["ws://127.0.0.1:7447".to_owned()])
            .is_err()
    );

    runtime
        .configure_simulator_relays(vec!["ws://127.0.0.1:7447".to_owned()])
        .expect("simulator relays");
    let simulator = runtime
        .sdk_relay_status()
        .expect("relay status")
        .expect("simulator profile");
    assert_eq!(simulator.profile, "simulator_local");
    assert_eq!(simulator.relays.len(), 1);
    assert_eq!(simulator.relays[0].access, "read_write");
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
    assert_eq!(device.relays.len(), 2);
    assert!(
        runtime
            .configure_device_relays(vec!["wss://127.0.0.1:7447".to_owned()])
            .is_err()
    );

    runtime
        .configure_public_blossom(vec!["https://media.example".to_owned()])
        .expect("public Blossom");
    assert_eq!(
        runtime.sdk_blossom_profile().expect("Blossom profile"),
        Some("public".to_owned())
    );
    runtime
        .configure_simulator_blossom(vec!["http://127.0.0.1:3100".to_owned()])
        .expect("simulator Blossom");
    runtime
        .configure_device_blossom(vec!["https://10.0.0.5:3100".to_owned()])
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
        food_status: None,
        media: Vec::new(),
    };
    runtime
        .phase1_validate_add_draft(add.clone(), 1_800_000_001)
        .expect("valid draft");
    let draft_id = "07".repeat(16);
    let saved = runtime
        .phase1_save_draft(
            draft_id.clone(),
            add,
            1_800_000_001,
            None,
            1_800_000_001_000,
        )
        .await
        .expect("saved draft");
    assert_eq!(saved.state, FfiOutboxState::Draft);
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
        .phase1_queue_draft(
            draft_id.clone(),
            saved.revision,
            FfiQueuePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                relay_urls: vec!["wss://write.example".to_owned()],
                satisfaction: FfiRelaySatisfaction::AllAccepted,
                delivery_deadline_unix_ms: 1_800_100_000_000,
                cancellation: FfiCancellationPolicy::LocalCooperative,
            },
            1_800_000_002_000,
        )
        .await
        .expect("queued draft");
    assert_eq!(queued.state, FfiOutboxState::Queued);
    let recovered = runtime
        .phase1_recover_draft_queue(draft_id.clone(), 1_800_000_003_000)
        .await
        .expect("recovered queue");
    assert_eq!(recovered.revision, queued.revision);
    let cancelled = runtime
        .phase1_cancel_draft(draft_id.clone(), recovered.revision, 1_800_000_004_000)
        .await
        .expect("cancelled draft");
    assert_eq!(cancelled.state, FfiOutboxState::Cancelled);

    let upload = FfiBlossomUploadInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION + 1,
        draft_id,
        expected_revision: cancelled.revision,
        media: FfiPreparedMediaInput {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            opaque_reference: "media:unused".to_owned(),
            file_descriptor: 0,
            url: "https://media.example/unused.png".to_owned(),
            sha256: "00".repeat(32),
            media_type: "image/png".to_owned(),
            byte_size: 1,
            width: 1,
            height: 1,
            alt: "unused".to_owned(),
            prepared_at_unix_s: 1_800_000_000,
        },
        authorization_content: "Upload exact image".to_owned(),
        authorization_created_at_unix_s: 1_800_000_000,
        authorization_lifetime_seconds: 60,
        operation_id: "08".repeat(16),
        artifact_id: "09".repeat(16),
        signing_deadline_unix_ms: 1_800_000_100_000,
        signing_cancellation: FfiCancellationPolicy::LocalCooperative,
        verified_at_unix_ms: 1_800_000_000_000,
        updated_at_unix_ms: 1_800_000_005_000,
    };
    let upload_error = runtime
        .phase1_upload_draft_media(upload.clone())
        .await
        .expect_err("unsupported upload schema");
    assert_eq!(upload_error.report().code, "unsupported_schema_version");
    let mut invalid_id_upload = upload;
    invalid_id_upload.schema_version = MOBILE_FFI_SCHEMA_VERSION;
    invalid_id_upload.draft_id = "not-a-draft-id".to_owned();
    assert_eq!(
        runtime
            .phase1_upload_draft_media(invalid_id_upload)
            .await
            .expect_err("invalid draft id")
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
