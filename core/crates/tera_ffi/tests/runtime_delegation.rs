use radroots_mobile_core::runtime::product_surface::{AddCommandType, TodayCardType};
use radroots_mobile_ffi::{RadrootsAppError, RadrootsRuntime};

const SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

#[tokio::test]
async fn native_boundary_delegates_the_complete_core_surface() {
    let runtime = RadrootsRuntime::new().expect("runtime");
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
        "memory"
    );

    assert!(!runtime.nostr_identity_has_selected_signing_identity());
    assert!(runtime.nostr_identity_selected_npub().is_none());
    assert!(
        runtime
            .nostr_identity_list()
            .expect("empty list")
            .is_empty()
    );
    assert!(
        runtime
            .nostr_identity_list_ids()
            .expect("empty identifiers")
            .is_empty()
    );
    assert!(
        runtime
            .nostr_identity_snapshot()
            .expect("empty snapshot")
            .identities
            .is_empty()
    );
    let validated = runtime
        .nostr_identity_validate_host_custody_secret(SECRET.to_owned())
        .expect("valid secret");
    let staged = runtime
        .nostr_identity_restore_host_custody_secret(
            SECRET.to_owned(),
            Some("staged".to_owned()),
            false,
        )
        .expect("staged identity");
    assert_eq!(validated.id, staged.id);
    let selected = runtime
        .nostr_identity_restore_host_custody_secret(
            SECRET.to_owned(),
            Some("selected".to_owned()),
            true,
        )
        .expect("selected identity");
    assert_eq!(
        runtime.nostr_identity_selected_npub(),
        Some(selected.public_key_npub.clone())
    );
    runtime
        .nostr_identity_select(selected.id.clone())
        .expect("select installed");
    assert!(runtime.nostr_identity_select("missing".to_owned()).is_err());
    runtime
        .nostr_identity_remove("missing".to_owned())
        .expect("removing missing identity is idempotent");
    runtime
        .nostr_identity_lock_host_custody_runtime()
        .expect("lock identity");
    runtime
        .nostr_identity_reset_host_custody_runtime()
        .expect("reset identity");

    assert!(runtime.nostr_set_default_relays(Vec::new()).is_err());
    assert!(runtime.nostr_connect_if_key_present().is_err());
    assert!(
        runtime
            .nostr_connection_status()
            .await
            .expect("unconfigured status")
            .last_error
            .is_none()
    );
    assert!(runtime.nostr_profile_for_self().await.is_err());
    assert!(
        runtime
            .nostr_post_profile(None, None, None, None)
            .await
            .is_err()
    );
    assert!(
        runtime
            .nostr_post_text_note("post".to_owned())
            .await
            .is_err()
    );
    assert!(runtime.nostr_fetch_text_notes(1, None).await.is_err());
    assert!(matches!(
        runtime
            .nostr_post_reply(
                "parent".to_owned(),
                "author".to_owned(),
                "reply".to_owned(),
                Some("different-root".to_owned()),
            )
            .await,
        Err(RadrootsAppError::Unsupported(_))
    ));

    assert_eq!(
        runtime.phase1_card_types(),
        vec![
            TodayCardType::Update,
            TodayCardType::PhotoUpdate,
            TodayCardType::Ask,
            TodayCardType::Event,
            TodayCardType::FoodAvailability,
        ]
    );
    assert_eq!(
        runtime.phase1_add_command_types(),
        vec![
            AddCommandType::CreateUpdate,
            AddCommandType::CreatePhotoUpdate,
            AddCommandType::CreateAsk,
            AddCommandType::CreateEvent,
            AddCommandType::CreateFoodAvailability,
        ]
    );
    let parity = runtime.phase1_card_add_parity();
    assert_eq!(parity.len(), 5);
    for (index, item) in parity.iter().enumerate() {
        assert_eq!(item.card_type, runtime.phase1_card_types()[index]);
        assert_eq!(
            item.add_command_type,
            runtime.phase1_add_command_types()[index]
        );
    }
    let local_network = runtime
        .phase1_local_network(
            "nearby".to_owned(),
            "Near me".to_owned(),
            vec!["wss://relay.example".to_owned()],
            Some("u10h".to_owned()),
            vec!["a".repeat(64)],
            1,
        )
        .expect("valid local network");
    assert_eq!(local_network.id, "nearby");
    assert!(
        runtime
            .phase1_local_network(
                "nearby".to_owned(),
                "Near me".to_owned(),
                Vec::new(),
                None,
                Vec::new(),
                1,
            )
            .is_err()
    );

    runtime.shutdown().await.expect("shutdown");
    assert!(matches!(
        runtime.sdk_storage_status().await,
        Err(RadrootsAppError::Sdk { .. })
    ));
}
