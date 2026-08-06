use thiserror::Error;

/// Versioned, secret-safe SDK failure exposed to mobile hosts.
#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct SdkErrorRecord {
    pub schema_version: u16,
    pub code: String,
    pub class: String,
    pub retryable: bool,
    pub recovery_actions: Vec<String>,
    pub operation_id: Option<String>,
    pub capability_id: Option<String>,
    pub message: String,
}

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

impl RadrootsAppError {
    pub(crate) fn from_sdk(error: radroots_sdk::Error) -> Self {
        let report = error.to_report();
        Self::Sdk {
            report: SdkErrorRecord {
                schema_version: report.schema_version(),
                code: report.code().as_str().to_owned(),
                class: debug_label(report.class()),
                retryable: report.retryable(),
                recovery_actions: report
                    .recovery_actions()
                    .iter()
                    .map(|action| debug_label(*action))
                    .collect(),
                operation_id: report.operation_id().map(|id| id.as_str().to_owned()),
                capability_id: report.capability_id().map(|id| id.as_str().to_owned()),
                message: report.message().as_str().to_owned(),
            },
        }
    }

    pub fn initialization(message: impl Into<String>) -> Self {
        Self::Initialization(message.into())
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

fn debug_label(value: impl std::fmt::Debug) -> String {
    let mut label = String::new();
    for (index, character) in format!("{value:?}").chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            label.push('_');
        }
        label.push(character.to_ascii_lowercase());
    }
    label
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{RadrootsAppError, SdkErrorRecord, debug_label};

    #[test]
    fn sdk_error_records_are_versioned_stable_and_secret_safe() {
        let error = radroots_sdk::ClientBuilder::new()
            .build()
            .expect_err("storage is required");
        let RadrootsAppError::Sdk { report } = RadrootsAppError::from_sdk(error) else {
            panic!("expected SDK report");
        };
        assert_eq!(
            report,
            SdkErrorRecord {
                schema_version: 1,
                code: "missing_storage".to_owned(),
                class: "capability".to_owned(),
                retryable: false,
                recovery_actions: vec!["configure_storage".to_owned()],
                operation_id: None,
                capability_id: Some("storage.canonical".to_owned()),
                message: "SDK storage capability is not configured".to_owned(),
            }
        );
        assert!(!format!("{report:?}").contains("source"));
    }

    #[test]
    fn debug_labels_use_stable_mobile_case() {
        assert_eq!(
            debug_label(SampleLabel::RetryAfterClose),
            "retry_after_close"
        );
    }

    #[test]
    fn public_error_constructors_preserve_typed_variants() {
        assert!(matches!(
            RadrootsAppError::initialization("init"),
            RadrootsAppError::Initialization(message) if message == "init"
        ));
        assert!(matches!(
            RadrootsAppError::runtime("runtime"),
            RadrootsAppError::Runtime(message) if message == "runtime"
        ));
        assert!(matches!(
            RadrootsAppError::unsupported("unsupported"),
            RadrootsAppError::Unsupported(message) if message == "unsupported"
        ));
        assert!(matches!(
            RadrootsAppError::internal("internal"),
            RadrootsAppError::Internal(message) if message == "internal"
        ));
    }

    #[derive(Debug)]
    enum SampleLabel {
        RetryAfterClose,
    }
}
