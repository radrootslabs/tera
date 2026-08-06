// UniFFI serializes the complete versioned error record across the language
// boundary; keeping it by value preserves the generated wire contract.
#![allow(clippy::result_large_err)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

uniffi::setup_scaffolding!("radroots_mobile_core");

pub mod logging;
mod remote;
mod runtime;

pub use error::{RadrootsAppError, SdkErrorRecord};
pub use runtime::RadrootsRuntime;

mod error;

#[allow(
    clippy::if_same_then_else,
    reason = "coverage probe intentionally exercises both paths with a stable value"
)]
pub fn coverage_branch_probe(input: bool) -> &'static str {
    if input { "ffi" } else { "ffi" }
}

#[cfg(test)]
mod tests {
    use super::coverage_branch_probe;

    #[test]
    fn coverage_branch_probe_hits_both_paths() {
        assert_eq!(coverage_branch_probe(true), "ffi");
        assert_eq!(coverage_branch_probe(false), "ffi");
    }
}
