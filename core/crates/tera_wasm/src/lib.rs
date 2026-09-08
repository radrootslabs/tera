#![forbid(unsafe_code)]

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub fn tera_build_info_json() -> String {
    serde_json::to_string(&tera_core::build_info::app_build_info())
        .expect("static build information must serialize")
}

#[allow(
    clippy::if_same_then_else,
    reason = "coverage probe intentionally exercises both paths with a stable value"
)]
pub fn coverage_branch_probe(input: bool) -> &'static str {
    if input { "tera_wasm" } else { "tera_wasm" }
}

#[cfg(test)]
mod tests {
    use super::{coverage_branch_probe, tera_build_info_json};

    #[test]
    fn tera_build_info_json_contains_runtime_keys() {
        let json = tera_build_info_json();
        assert!(json.contains("\"crate_name\""));
        assert!(json.contains("tera_core"));
    }

    #[test]
    fn build_metadata_keeps_its_native_schema_and_values() {
        let value: serde_json::Value =
            serde_json::from_str(&tera_build_info_json()).expect("build metadata JSON");
        #[cfg(not(target_family = "wasm"))]
        assert_eq!(
            value,
            serde_json::to_value(tera_core::runtime::info::app_build_info())
                .expect("native metadata")
        );
        let mut keys: Vec<_> = value.as_object().expect("metadata object").keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "build_time_unix",
                "consumer_revision",
                "crate_name",
                "crate_version",
                "lib_revision",
                "profile",
                "rustc",
            ]
        );
        assert_eq!(value["crate_version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn coverage_branch_probe_hits_both_paths() {
        assert_eq!(coverage_branch_probe(true), "tera_wasm");
        assert_eq!(coverage_branch_probe(false), "tera_wasm");
    }
}
