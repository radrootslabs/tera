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

    pub fn sdk_storage_status(&self) -> Result<SdkStorageStatusRecord, RadrootsAppError> {
        #[cfg(feature = "rt")]
        {
            let executor = self
                .executor
                .lock()
                .map_err(|_| RadrootsAppError::runtime("SDK executor lock is unavailable"))?;
            let executor = executor
                .as_ref()
                .ok_or_else(|| RadrootsAppError::runtime("SDK runtime is closed"))?;
            let status = executor
                .block_on(self.client.storage_status())
                .map_err(RadrootsAppError::from_sdk)?;
            Ok(SdkStorageStatusRecord {
                backend: format!("{:?}", status.backend()).to_ascii_lowercase(),
                open_mode: format!("{:?}", status.open_mode()).to_ascii_lowercase(),
                shutdown: format!("{:?}", status.shutdown()).to_ascii_lowercase(),
                integrity: format!("{:?}", status.integrity().health()).to_ascii_lowercase(),
            })
        }
        #[cfg(not(feature = "rt"))]
        {
            Err(RadrootsAppError::unsupported(
                "SDK storage status requires a host async executor",
            ))
        }
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
mod tests {
    use super::RadrootsRuntime;

    #[test]
    fn sdk_records_are_stable_and_storage_is_memory_backed() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        let capabilities = runtime.sdk_capabilities();
        assert!(capabilities.iter().any(|capability| {
            capability.id == "storage.canonical"
                && capability.configured
                && capability.availability == "available"
                && capability.maturity == "stable"
        }));
        let status = runtime.sdk_storage_status().expect("storage status");
        assert_eq!(status.backend, "memory");
        assert_eq!(status.integrity, "healthy");
        runtime.stop();
        assert!(runtime.sdk_storage_status().is_err());
    }
}
