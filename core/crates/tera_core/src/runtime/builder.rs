use crate::{RadrootsAppError, RadrootsRuntime};

/// Host-owned construction boundary for the shared SDK-backed runtime.
#[derive(Default)]
pub struct RuntimeBuilder;

impl RuntimeBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn build(self) -> Result<RadrootsRuntime, RadrootsAppError> {
        RadrootsRuntime::new()
    }
}
