use radroots_mobile_core::{
    RadrootsRuntime,
    runtime::{
        builder::RuntimeBuilder,
        store::{MobileUserStoreConfig, ProtectedDataAvailability},
    },
};

pub const PUBLIC_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
pub const GENERATION: &str = "0303030303030303030303030303030303030303030303030303030303030303";

pub fn store(root: &std::path::Path) -> MobileUserStoreConfig {
    let store = MobileUserStoreConfig::from_encoded(
        root,
        PUBLIC_KEY,
        GENERATION,
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .expect("store config");
    std::fs::create_dir_all(store.owner_directory()).expect("owner directory");
    store
}

#[allow(dead_code)]
pub async fn runtime() -> (tempfile::TempDir, RadrootsRuntime) {
    let root = tempfile::tempdir().expect("tempdir");
    let runtime = RuntimeBuilder::new(store(root.path()))
        .build()
        .await
        .expect("runtime");
    (root, runtime)
}
