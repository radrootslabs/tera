// Mobile errors retain the complete stable SDK report. Preserving that typed
// value is more important than optimizing the Rust enum's in-process size.
#![allow(clippy::result_large_err)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod error;
#[cfg(test)]
mod provenance;
pub mod runtime;

pub use error::{RadrootsAppError, SdkErrorRecord, StoreErrorRecord};
pub use runtime::RadrootsRuntime;
