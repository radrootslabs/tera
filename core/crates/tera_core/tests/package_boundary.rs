const MANIFEST: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const ERROR: &str = include_str!("../src/error.rs");
const RUNTIME: &str = include_str!("../src/runtime/mod.rs");
const APP_INFO: &str = include_str!("../src/runtime/app_info.rs");
const INFO: &str = include_str!("../src/runtime/info.rs");
const PRODUCT_SURFACE: &str = include_str!("../src/runtime/product_surface.rs");
const PRODUCT_AUTHORING: &str = include_str!("../src/runtime/product_surface/authoring.rs");
const PRODUCT_CONTEXT: &str = include_str!("../src/runtime/product_surface/context.rs");
const PRODUCT_CURSOR: &str = include_str!("../src/runtime/product_surface/cursor.rs");
const PRODUCT_IDENTITY: &str = include_str!("../src/runtime/product_surface/identity.rs");
const PRODUCT_MODEL: &str = include_str!("../src/runtime/product_surface/model.rs");
const PRODUCT_OUTBOX: &str = include_str!("../src/runtime/product_surface/outbox.rs");
const PRODUCT_PROJECTION: &str = include_str!("../src/runtime/product_surface/projection.rs");
const PRODUCT_RANKING: &str = include_str!("../src/runtime/product_surface/ranking.rs");
const SDK: &str = include_str!("../src/runtime/sdk.rs");
const BUILDER: &str = include_str!("../src/runtime/builder.rs");
const STORE: &str = include_str!("../src/runtime/store.rs");

#[test]
fn core_owns_no_uniffi_or_process_global_logging_policy() {
    for (name, source) in [
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", LIB),
        ("src/error.rs", ERROR),
        ("src/runtime/mod.rs", RUNTIME),
        ("src/runtime/app_info.rs", APP_INFO),
        ("src/runtime/info.rs", INFO),
        ("src/runtime/product_surface.rs", PRODUCT_SURFACE),
        (
            "src/runtime/product_surface/authoring.rs",
            PRODUCT_AUTHORING,
        ),
        ("src/runtime/product_surface/context.rs", PRODUCT_CONTEXT),
        ("src/runtime/product_surface/cursor.rs", PRODUCT_CURSOR),
        ("src/runtime/product_surface/identity.rs", PRODUCT_IDENTITY),
        ("src/runtime/product_surface/model.rs", PRODUCT_MODEL),
        ("src/runtime/product_surface/outbox.rs", PRODUCT_OUTBOX),
        (
            "src/runtime/product_surface/projection.rs",
            PRODUCT_PROJECTION,
        ),
        ("src/runtime/product_surface/ranking.rs", PRODUCT_RANKING),
        ("src/runtime/sdk.rs", SDK),
    ] {
        assert!(
            !source.to_ascii_lowercase().contains("uniffi"),
            "{name} contains UniFFI boundary policy"
        );
        assert!(
            !source.contains("tracing_subscriber") && !source.contains("set_global_default"),
            "{name} contains process-global logging policy"
        );
    }
}

#[test]
fn mobile_runtime_has_no_secret_taking_or_local_signer_slot_surface() {
    for source in [
        MANIFEST,
        RUNTIME,
        BUILDER,
        PRODUCT_AUTHORING,
        PRODUCT_OUTBOX,
    ] {
        assert!(!source.contains("signing::Slot"));
        assert!(!source.contains("secret_key: String"));
        assert!(!source.contains("Provider::slot"));
    }
    assert!(!MANIFEST.contains("radroots_sdk/local-signing"));
}

#[test]
fn production_runtime_requires_validated_sqlite_and_memory_is_test_only() {
    assert!(MANIFEST.contains("radroots_sdk = { workspace = true, features = [\"sqlite\"] }"));
    assert_eq!(BUILDER.matches("ClientBuilder::sqlite").count(), 1);
    assert!(!BUILDER.contains("memory_default") && !STORE.contains("memory_default"));
    assert!(RUNTIME.contains("#[cfg(test)]\n    pub(crate) fn test_memory()"));
    assert!(!RUNTIME.contains("pub fn new()"));
}

#[test]
fn mobile_core_reuses_the_final_sdk_evidence_vocabulary() {
    for required in [
        "RadrootsRhiEvidenceReportV1",
        "RadrootsTradeEvidenceCoverageV1",
        "RadrootsTradeEvidenceManifestV1",
        "RadrootsTradeEvidenceOutcomeV1",
    ] {
        assert!(
            SDK.contains(required),
            "missing SDK evidence type `{required}`"
        );
    }
    for forbidden in ["SecretKey", "sign_event", "publish_event", "tokio::spawn"] {
        assert!(
            !SDK.contains(forbidden),
            "mobile evidence projection gained forbidden authority `{forbidden}`"
        );
    }
}
