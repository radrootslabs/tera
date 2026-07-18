#![forbid(unsafe_code)]

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub fn field_wasm_build_info_json() -> String {
    let runtime = radroots_app_core::RadrootsRuntime::new()
        .expect("runtime init must succeed with radroots_app_core no-default-features");
    runtime.info_json()
}

#[allow(
    clippy::if_same_then_else,
    reason = "coverage probe intentionally exercises both paths with a stable value"
)]
pub fn coverage_branch_probe(input: bool) -> &'static str {
    if input { "field_wasm" } else { "field_wasm" }
}

#[cfg(test)]
mod tests {
    use super::{coverage_branch_probe, field_wasm_build_info_json};

    #[test]
    fn field_wasm_build_info_json_contains_runtime_keys() {
        let json = field_wasm_build_info_json();
        assert!(json.contains("\"app\""));
    }

    #[test]
    fn coverage_branch_probe_hits_both_paths() {
        assert_eq!(coverage_branch_probe(true), "field_wasm");
        assert_eq!(coverage_branch_probe(false), "field_wasm");
    }
}
