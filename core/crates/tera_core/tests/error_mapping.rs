#![cfg(feature = "nostr-client")]

use std::{sync::mpsc, time::Duration};

use radroots_app_core::runtime::trade_listing::TradeListingDraft;
use radroots_app_core::{RadrootsAppError, RadrootsRuntime};

#[test]
fn invalid_host_custody_secret_maps_to_identity_error() {
    let runtime = RadrootsRuntime::new().expect("runtime");

    let err = runtime
        .nostr_identity_validate_host_custody_secret("not-a-secret".to_string())
        .expect_err("invalid secret should fail");

    assert!(matches!(err, RadrootsAppError::Identity(_)));
}

#[test]
fn uninitialized_nostr_publish_maps_to_relay_error() {
    let runtime = RadrootsRuntime::new().expect("runtime");

    let err = runtime
        .nostr_post_text_note("hello".to_string())
        .expect_err("uninitialized nostr should fail");

    assert!(matches!(
        err,
        RadrootsAppError::Relay(message) if message == "nostr not initialized"
    ));
}

#[test]
fn profile_read_without_identity_maps_to_identity_error() {
    let runtime = RadrootsRuntime::new().expect("runtime");

    let err = runtime
        .nostr_profile_for_self()
        .expect_err("missing identity should fail");

    assert!(matches!(err, RadrootsAppError::Identity(_)));
}

#[test]
fn profile_read_without_initialized_nostr_maps_to_relay_error() {
    let runtime = RadrootsRuntime::new().expect("runtime");
    let identity = radroots_identity::RadrootsIdentity::generate();
    runtime
        .nostr_identity_restore_host_custody_secret(
            identity.secret_key_hex(),
            Some("field".to_string()),
            true,
        )
        .expect("restore identity");

    let err = runtime
        .nostr_profile_for_self()
        .expect_err("uninitialized nostr should fail");

    assert!(matches!(
        err,
        RadrootsAppError::Relay(message) if message == "nostr not initialized"
    ));
}

#[test]
fn post_stream_read_without_started_stream_returns_no_data() {
    let runtime = RadrootsRuntime::new().expect("runtime");

    let event = runtime
        .nostr_next_post_event()
        .expect("missing stream should be a no-data state");

    assert!(event.is_none());
}

#[test]
fn trade_listing_publish_with_initialized_nostr_does_not_relock_runtime() {
    let runtime = RadrootsRuntime::new().expect("runtime");
    let identity = radroots_identity::RadrootsIdentity::generate();
    let public_key_hex = identity.public_key_hex();
    runtime
        .nostr_identity_restore_host_custody_secret(
            identity.secret_key_hex(),
            Some("field".to_string()),
            true,
        )
        .expect("restore identity");
    runtime
        .nostr_set_default_relays(Vec::new())
        .expect("initialize nostr manager");
    let draft = TradeListingDraft {
        listing_id: Some("AAAAAAAAAAAAAAAAAAAAAg".to_string()),
        farm_pubkey: public_key_hex,
        farm_d_tag: "AAAAAAAAAAAAAAAAAAAAAQ".to_string(),
        title: "Carrots".to_string(),
        description: "Fresh carrots".to_string(),
        category: "produce".to_string(),
        bin_display_amount: "1".to_string(),
        bin_display_unit: "lb".to_string(),
        unit_price: "3.50".to_string(),
        currency: "USD".to_string(),
        bin_label: None,
        bin_id: Some("bin-1".to_string()),
        inventory: "10".to_string(),
        delivery_method: "pickup".to_string(),
        location_primary: "farm stand".to_string(),
        location_city: Some("Asheville".to_string()),
        location_region: None,
        location_country: None,
        location_geohash: "9q8yy".to_string(),
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = runtime.trade_listing_publish(draft);
        let _ = tx.send(result);
    });

    let result = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("publish must return instead of re-locking the runtime");

    match result {
        Ok(_) | Err(RadrootsAppError::Relay(_)) => {}
        other => panic!("unexpected publish result: {other:?}"),
    }
}
