// UniFFI errors are serialized value contracts. Keeping the complete stable
// SDK report on the error is more important than optimizing the Rust enum's
// in-process size; mobile calls cross this boundary by value in all cases.
#![allow(clippy::result_large_err)]

uniffi::setup_scaffolding!("radroots");

pub mod error;
pub mod logging;
pub mod runtime;

pub use error::{RadrootsAppError, SdkErrorRecord};
pub use runtime::RadrootsRuntime;
