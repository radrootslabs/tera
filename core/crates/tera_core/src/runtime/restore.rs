//! Application restore binding. Database replacement remains with the canonical
//! owner; the native host owns durable guard and media-file admission.

mod barrier;
mod error;
mod model;
mod orchestration;
mod reconciliation;
mod startup;
mod status;
pub use status::{RestorePendingTarget, RestorePhase, RestoreStatus};
mod review_record;
pub(crate) use barrier::{BARRIER_SCHEMA, barrier_metadata_is_valid};
pub use orchestration::{RestoreHost, restore_application_backup};
pub(crate) use review_record::{REVIEW_SCHEMA, review_metadata_is_valid};
pub use review_record::{RestoreObservation, RestoreTargetReview};

pub use error::RestoreError;
pub use model::{ApplicationRestoreGuard, RESTORE_GUARD_MAX_BYTES, RestoreRequest};

#[cfg(test)]
mod fault_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod orchestration_tests;
#[cfg(test)]
mod reconciliation_tests;
#[cfg(test)]
mod relay_test_support;
#[cfg(test)]
mod test_support;
