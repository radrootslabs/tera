use radroots_mobile_ffi::{ProtectedDataAvailability, RadrootsRuntime};

pub const PUBLIC_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
pub const GENERATION: &str = "0404040404040404040404040404040404040404040404040404040404040404";

pub fn prepare(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("radroots").join("users").join(PUBLIC_KEY))
        .expect("owner directory");
}

pub async fn runtime() -> (tempfile::TempDir, RadrootsRuntime) {
    let root = tempfile::tempdir().expect("tempdir");
    prepare(root.path());
    let runtime = RadrootsRuntime::new(
        root.path().to_string_lossy().into_owned(),
        PUBLIC_KEY.to_owned(),
        GENERATION.to_owned(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .expect("runtime");
    (root, runtime)
}
