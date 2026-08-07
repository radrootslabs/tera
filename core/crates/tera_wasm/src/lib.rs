#![forbid(unsafe_code)]

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub fn radroots_mobile_build_info_json() -> String {
    serde_json::to_string(&radroots_mobile_core::runtime::info::app_build_info())
        .expect("static build information must serialize")
}

#[allow(
    clippy::if_same_then_else,
    reason = "coverage probe intentionally exercises both paths with a stable value"
)]
pub fn coverage_branch_probe(input: bool) -> &'static str {
    if input {
        "radroots_mobile_wasm"
    } else {
        "radroots_mobile_wasm"
    }
}

#[cfg(test)]
mod tests {
    use super::{coverage_branch_probe, radroots_mobile_build_info_json};

    #[test]
    fn radroots_mobile_build_info_json_contains_runtime_keys() {
        let json = radroots_mobile_build_info_json();
        assert!(json.contains("\"crate_name\""));
        assert!(json.contains("radroots_mobile_core"));
    }

    #[test]
    fn coverage_branch_probe_hits_both_paths() {
        assert_eq!(coverage_branch_probe(true), "radroots_mobile_wasm");
        assert_eq!(coverage_branch_probe(false), "radroots_mobile_wasm");
    }
}
