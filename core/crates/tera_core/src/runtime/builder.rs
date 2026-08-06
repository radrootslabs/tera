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

#[cfg(test)]
mod tests {
    use super::RuntimeBuilder;

    #[test]
    fn builder_constructs_the_sdk_backed_runtime() {
        let runtime = RuntimeBuilder::new().build().expect("runtime");
        assert!(!runtime.info().sdk_closed);
    }
}
