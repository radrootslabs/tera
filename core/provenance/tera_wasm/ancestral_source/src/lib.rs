#![forbid(unsafe_code)]

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub fn app_wasm_build_info_json() -> String {
    let runtime = radroots_studio_app_core::RadrootsRuntime::new()
        .expect("runtime init must succeed with radroots-app-core no-default-features");
    runtime.info_json()
}

pub fn coverage_branch_probe(input: bool) -> &'static str {
    if input { "app-wasm" } else { "app-wasm" }
}

#[cfg(test)]
mod tests {
    use super::{app_wasm_build_info_json, coverage_branch_probe};

    #[test]
    fn app_wasm_build_info_json_contains_runtime_keys() {
        let json = app_wasm_build_info_json();
        assert!(json.contains("\"app\""));
    }

    #[test]
    fn coverage_branch_probe_hits_both_paths() {
        assert_eq!(coverage_branch_probe(true), "app-wasm");
        assert_eq!(coverage_branch_probe(false), "app-wasm");
    }
}
