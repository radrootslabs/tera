// Mobile errors retain the complete stable SDK report. Preserving that typed
// value is more important than optimizing the Rust enum's in-process size.
#![allow(clippy::result_large_err)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod build_info;
#[cfg(not(target_family = "wasm"))]
pub mod error;
#[cfg(test)]
mod provenance;
#[cfg(not(target_family = "wasm"))]
pub mod runtime;

#[cfg(not(target_family = "wasm"))]
pub use error::{RadrootsAppError, SdkErrorRecord, StoreErrorRecord};
#[cfg(not(target_family = "wasm"))]
pub use runtime::RadrootsRuntime;
