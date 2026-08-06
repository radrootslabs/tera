// UniFFI errors are serialized value contracts. Keeping the complete stable
// SDK report on the error is more important than optimizing the Rust enum's
// in-process size; mobile calls cross this boundary by value in all cases.
#![allow(clippy::result_large_err)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

uniffi::setup_scaffolding!("radroots_mobile_core");

pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub mod logging;
pub mod runtime;

pub use error::{RadrootsAppError, SdkErrorRecord};
pub use runtime::RadrootsRuntime;
