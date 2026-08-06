use radroots_sdk::capability::{Availability, Maturity};

use super::RadrootsRuntime;
use crate::RadrootsAppError;

#[derive(Clone, Debug, uniffi::Record)]
pub struct SdkCapabilityRecord {
    pub id: String,
    pub compiled: bool,
    pub configured: bool,
    pub availability: String,
    pub maturity: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SdkStorageStatusRecord {
    pub backend: String,
    pub open_mode: String,
    pub shutdown: String,
    pub integrity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct SdkShutdownRecord {
    pub state: String,
    pub already_closed: bool,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    pub fn sdk_capabilities(&self) -> Vec<SdkCapabilityRecord> {
        self.client
            .capabilities()
            .iter()
            .map(|status| SdkCapabilityRecord {
                id: status.id().as_str().to_owned(),
                compiled: status.is_compiled(),
                configured: status.is_configured(),
                availability: availability_label(status.availability()).to_owned(),
                maturity: maturity_label(status.maturity()).to_owned(),
            })
            .collect()
    }

    pub async fn sdk_storage_status(&self) -> Result<SdkStorageStatusRecord, RadrootsAppError> {
        let status = self
            .client
            .storage_status()
            .await
            .map_err(RadrootsAppError::from_sdk)?;
        Ok(SdkStorageStatusRecord {
            backend: status.backend().as_str().to_owned(),
            open_mode: status.open_mode().as_str().to_owned(),
            shutdown: status.shutdown().as_str().to_owned(),
            integrity: status.integrity().health().as_str().to_owned(),
        })
    }
}

const fn availability_label(value: Availability) -> &'static str {
    match value {
        Availability::Available => "available",
        Availability::Degraded => "degraded",
        Availability::Unavailable => "unavailable",
        Availability::Unsupported => "unsupported",
    }
}

const fn maturity_label(value: Maturity) -> &'static str {
    match value {
        Maturity::Stable => "stable",
        Maturity::Preview => "preview",
        Maturity::Experimental => "experimental",
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::RadrootsRuntime;

    #[tokio::test]
    async fn sdk_records_are_stable_and_storage_is_memory_backed() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        let capabilities = runtime.sdk_capabilities();
        assert!(capabilities.iter().any(|capability| {
            capability.id == "storage.canonical"
                && capability.configured
                && capability.availability == "available"
                && capability.maturity == "stable"
        }));
        let status = runtime.sdk_storage_status().await.expect("storage status");
        assert_eq!(status.backend, "memory");
        assert_eq!(status.integrity, "healthy");
        runtime.shutdown().await.expect("shutdown");
        assert!(runtime.sdk_storage_status().await.is_err());
    }
}
