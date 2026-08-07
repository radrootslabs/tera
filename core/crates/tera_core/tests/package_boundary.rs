const MANIFEST: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const ERROR: &str = include_str!("../src/error.rs");
const RUNTIME: &str = include_str!("../src/runtime/mod.rs");
const APP_INFO: &str = include_str!("../src/runtime/app_info.rs");
const INFO: &str = include_str!("../src/runtime/info.rs");
const KEY_MANAGEMENT: &str = include_str!("../src/runtime/key_management.rs");
const NOSTR: &str = include_str!("../src/runtime/nostr.rs");
const PRODUCT_SURFACE: &str = include_str!("../src/runtime/product_surface.rs");
const PRODUCT_CONTEXT: &str = include_str!("../src/runtime/product_surface/context.rs");
const PRODUCT_CURSOR: &str = include_str!("../src/runtime/product_surface/cursor.rs");
const PRODUCT_IDENTITY: &str = include_str!("../src/runtime/product_surface/identity.rs");
const PRODUCT_MODEL: &str = include_str!("../src/runtime/product_surface/model.rs");
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
        ("src/runtime/key_management.rs", KEY_MANAGEMENT),
        ("src/runtime/nostr.rs", NOSTR),
        ("src/runtime/product_surface.rs", PRODUCT_SURFACE),
        ("src/runtime/product_surface/context.rs", PRODUCT_CONTEXT),
        ("src/runtime/product_surface/cursor.rs", PRODUCT_CURSOR),
        ("src/runtime/product_surface/identity.rs", PRODUCT_IDENTITY),
        ("src/runtime/product_surface/model.rs", PRODUCT_MODEL),
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
fn production_runtime_requires_validated_sqlite_and_memory_is_test_only() {
    assert!(MANIFEST.contains("radroots_sdk = { workspace = true, features = [\"sqlite\"] }"));
    assert_eq!(BUILDER.matches("ClientBuilder::sqlite").count(), 1);
    assert!(!BUILDER.contains("memory_default") && !STORE.contains("memory_default"));
    assert!(RUNTIME.contains("#[cfg(test)]\n    pub(crate) fn test_memory()"));
    assert!(!RUNTIME.contains("pub fn new()"));
}
