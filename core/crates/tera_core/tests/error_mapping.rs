#![cfg(feature = "nostr-client")]

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
fn retired_trade_operations_map_to_unsupported_error() {
    let runtime = RadrootsRuntime::new().expect("runtime");

    let validation_err = runtime
        .trade_listing_send_validation_request(
            "event".to_string(),
            "seller".to_string(),
            "listing".to_string(),
            "recipient".to_string(),
        )
        .expect_err("retired validation request should fail");
    let messages_err = runtime
        .trade_listing_fetch_messages("listing".to_string(), 10, None)
        .expect_err("retired message fetch should fail");

    assert!(matches!(validation_err, RadrootsAppError::Unsupported(_)));
    assert!(matches!(messages_err, RadrootsAppError::Unsupported(_)));
}
