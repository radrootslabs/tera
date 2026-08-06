use thiserror::Error;

pub use radroots_mobile_core::SdkErrorRecord;

/// Versioned, secret-safe failure exposed across the native language boundary.
#[derive(Debug, Error, uniffi::Error)]
pub enum RadrootsAppError {
    #[error("initialization: {0}")]
    Initialization(String),
    #[error("sdk: {report:?}")]
    Sdk { report: SdkErrorRecord },
    #[error("runtime: {0}")]
    Runtime(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl From<radroots_mobile_core::RadrootsAppError> for RadrootsAppError {
    fn from(error: radroots_mobile_core::RadrootsAppError) -> Self {
        match error {
            radroots_mobile_core::RadrootsAppError::Initialization(message) => {
                Self::Initialization(message)
            }
            radroots_mobile_core::RadrootsAppError::Sdk { report } => Self::Sdk { report },
            radroots_mobile_core::RadrootsAppError::Runtime(message) => Self::Runtime(message),
            radroots_mobile_core::RadrootsAppError::Unsupported(message) => {
                Self::Unsupported(message)
            }
            radroots_mobile_core::RadrootsAppError::Internal(message) => Self::Internal(message),
        }
    }
}

impl RadrootsAppError {
    pub(crate) fn initialization(message: impl Into<String>) -> Self {
        Self::Initialization(message.into())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::RadrootsAppError;

    #[test]
    fn every_core_error_variant_maps_without_string_erasure() {
        assert!(matches!(
            RadrootsAppError::from(radroots_mobile_core::RadrootsAppError::initialization(
                "initialization"
            )),
            RadrootsAppError::Initialization(message) if message == "initialization"
        ));
        assert!(matches!(
            RadrootsAppError::from(radroots_mobile_core::RadrootsAppError::runtime("runtime")),
            RadrootsAppError::Runtime(message) if message == "runtime"
        ));
        assert!(matches!(
            RadrootsAppError::from(radroots_mobile_core::RadrootsAppError::unsupported(
                "unsupported"
            )),
            RadrootsAppError::Unsupported(message) if message == "unsupported"
        ));
        assert!(matches!(
            RadrootsAppError::from(radroots_mobile_core::RadrootsAppError::internal("internal")),
            RadrootsAppError::Internal(message) if message == "internal"
        ));
    }
}
