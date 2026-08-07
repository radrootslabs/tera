use radroots_mobile_core::runtime::product_surface::{AddCommandType, TodayCardType};
use radroots_mobile_ffi::RadrootsAppError;

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
