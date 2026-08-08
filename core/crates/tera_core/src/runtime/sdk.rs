use radroots_sdk::capability::{Availability, Maturity};

use super::RadrootsRuntime;
use crate::RadrootsAppError;

#[derive(Clone, Debug)]
pub struct SdkCapabilityRecord {
    pub id: String,
    pub compiled: bool,
    pub configured: bool,
    pub availability: String,
    pub maturity: String,
}

#[derive(Clone, Debug)]
pub struct SdkStorageStatusRecord {
    pub backend: String,
    pub open_mode: String,
    pub shutdown: String,
    pub integrity: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkRelayStatusRecord {
    pub relay_url: String,
    pub access: String,
    pub read_state: String,
    pub write_state: String,
    pub read_last_attempt_unix_ms: Option<u64>,
    pub write_last_attempt_unix_ms: Option<u64>,
    pub read_next_attempt_unix_ms: Option<u64>,
    pub write_next_attempt_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkRelayStatusReportRecord {
    pub profile: String,
    pub state: String,
    pub read_availability: String,
    pub write_availability: String,
    pub relays: Vec<SdkRelayStatusRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkBlossomConfigurationRecord {
    pub host_kind: String,
    pub endpoint_authority: String,
    pub primary_origin: String,
    pub fallback_origins: Vec<String>,
    pub config_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkShutdownRecord {
    pub state: String,
    pub already_closed: bool,
}

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

    /// Installs a validated public relay profile without probing it.
    #[cfg(feature = "mobile-social")]
    pub fn configure_public_relays(
        &self,
        writable_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.configure_relay_profile(
            radroots_sdk::transport::RelayProfile::public(writable_relays)
                .map_err(|error| RadrootsAppError::runtime(error.to_string()))?,
        )
    }

    /// Installs an exact-loopback simulator profile without probing it.
    #[cfg(feature = "mobile-social")]
    pub fn configure_simulator_relays(
        &self,
        loopback_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.configure_relay_profile(
            radroots_sdk::transport::RelayProfile::simulator(loopback_relays)
                .map_err(|error| RadrootsAppError::runtime(error.to_string()))?,
        )
    }

    /// Installs an explicit physical-device TLS relay profile without probing it.
    #[cfg(feature = "mobile-social")]
    pub fn configure_device_relays(
        &self,
        writable_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.configure_relay_profile(
            radroots_sdk::transport::RelayProfile::device(writable_relays)
                .map_err(|error| RadrootsAppError::runtime(error.to_string()))?,
        )
    }

    #[cfg(feature = "mobile-social")]
    fn configure_relay_profile(
        &self,
        profile: radroots_sdk::transport::RelayProfile,
    ) -> Result<(), RadrootsAppError> {
        self.client
            .configure_nostr(profile)
            .map_err(RadrootsAppError::from_sdk)
    }

    /// Installs one canonical inert Blossom configuration without probing it.
    #[cfg(feature = "mobile-social")]
    pub fn configure_blossom(
        &self,
        host_kind: radroots_sdk::transport::BlossomHostKind,
        endpoint_authority: radroots_sdk::transport::BlossomEndpointAuthority,
        primary_origin: String,
        fallback_origins: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.configure_blossom_profile(
            radroots_sdk::transport::BlossomProfile::new(
                host_kind,
                endpoint_authority,
                primary_origin,
                fallback_origins,
            )
            .map_err(|error| RadrootsAppError::runtime(error.code().to_owned()))?,
        )
    }

    #[cfg(feature = "mobile-social")]
    fn configure_blossom_profile(
        &self,
        profile: radroots_sdk::transport::BlossomProfile,
    ) -> Result<(), RadrootsAppError> {
        self.client
            .configure_blossom(radroots_sdk::transport::BlossomConfig::from_profile(
                profile,
            ))
            .map_err(RadrootsAppError::from_sdk)
    }

    /// Returns the configured adapter slot for Rust-owned media binding.
    #[cfg(feature = "mobile-social")]
    pub fn sdk_blossom_slot(
        &self,
    ) -> Result<Option<radroots_sdk::transport::BlossomSlot>, RadrootsAppError> {
        self.client
            .blossom()
            .map(|slot| slot.cloned())
            .map_err(RadrootsAppError::from_sdk)
    }

    /// Returns the complete inert Blossom configuration, when configured.
    #[cfg(feature = "mobile-social")]
    pub fn sdk_blossom_configuration(
        &self,
    ) -> Result<Option<SdkBlossomConfigurationRecord>, RadrootsAppError> {
        let configuration = self
            .client
            .blossom()
            .map_err(RadrootsAppError::from_sdk)?
            .and_then(radroots_sdk::transport::BlossomSlot::configuration);
        Ok(
            configuration.map(|(profile, fingerprint)| SdkBlossomConfigurationRecord {
                host_kind: blossom_host_kind_label(profile.host_kind()).to_owned(),
                endpoint_authority: blossom_authority_label(profile.authority()).to_owned(),
                primary_origin: profile.primary().origin().to_owned(),
                fallback_origins: profile
                    .fallbacks()
                    .iter()
                    .map(|endpoint| endpoint.origin().to_owned())
                    .collect(),
                config_fingerprint: fingerprint.to_hex(),
            }),
        )
    }

    /// Returns passive relay evidence without DNS, socket, or probe work.
    #[cfg(feature = "mobile-social")]
    pub fn sdk_relay_status(&self) -> Result<Option<SdkRelayStatusReportRecord>, RadrootsAppError> {
        let report = self
            .client
            .nostr_status()
            .map_err(RadrootsAppError::from_sdk)?;
        Ok(report.map(|report| SdkRelayStatusReportRecord {
            profile: relay_profile_label(report.profile_kind()).to_owned(),
            state: relay_aggregate_label(report.state()).to_owned(),
            read_availability: transport_availability_label(report.read_availability()).to_owned(),
            write_availability: transport_availability_label(report.write_availability())
                .to_owned(),
            relays: report
                .relays()
                .iter()
                .map(|relay| SdkRelayStatusRecord {
                    relay_url: relay.endpoint().url().to_string(),
                    access: if relay.endpoint().access().can_write() {
                        "read_write"
                    } else {
                        "read_only"
                    }
                    .to_owned(),
                    read_state: relay_evidence_label(relay.read().state()).to_owned(),
                    write_state: relay_evidence_label(relay.write().state()).to_owned(),
                    read_last_attempt_unix_ms: relay.read().last_attempt_unix_ms(),
                    write_last_attempt_unix_ms: relay.write().last_attempt_unix_ms(),
                    read_next_attempt_unix_ms: relay.read().next_attempt_unix_ms(),
                    write_next_attempt_unix_ms: relay.write().next_attempt_unix_ms(),
                })
                .collect(),
        }))
    }
}

#[cfg(feature = "mobile-social")]
const fn blossom_host_kind_label(value: radroots_sdk::transport::BlossomHostKind) -> &'static str {
    match value {
        radroots_sdk::transport::BlossomHostKind::Native => "native",
        radroots_sdk::transport::BlossomHostKind::Simulator => "simulator",
        radroots_sdk::transport::BlossomHostKind::PhysicalDevice => "physical_device",
        _ => "unknown",
    }
}

#[cfg(feature = "mobile-social")]
const fn blossom_authority_label(
    value: radroots_sdk::transport::BlossomEndpointAuthority,
) -> &'static str {
    match value {
        radroots_sdk::transport::BlossomEndpointAuthority::PublicWebPki => "public_webpki",
        radroots_sdk::transport::BlossomEndpointAuthority::LoopbackDevelopment => {
            "loopback_development"
        }
        radroots_sdk::transport::BlossomEndpointAuthority::PrivateNetworkDevelopment => {
            "private_network_development"
        }
        _ => "unknown",
    }
}

#[cfg(feature = "mobile-social")]
const fn relay_evidence_label(value: radroots_sdk::transport::RelayEvidenceState) -> &'static str {
    match value {
        radroots_sdk::transport::RelayEvidenceState::Unsupported => "unsupported",
        radroots_sdk::transport::RelayEvidenceState::Unobserved => "unobserved",
        radroots_sdk::transport::RelayEvidenceState::Connecting => "connecting",
        radroots_sdk::transport::RelayEvidenceState::Available => "available",
        radroots_sdk::transport::RelayEvidenceState::Unavailable => "unavailable",
        _ => "unknown",
    }
}

#[cfg(feature = "mobile-social")]
const fn relay_profile_label(value: radroots_sdk::transport::RelayProfileKind) -> &'static str {
    match value {
        radroots_sdk::transport::RelayProfileKind::Public => "public",
        radroots_sdk::transport::RelayProfileKind::Simulator => "simulator_local",
        radroots_sdk::transport::RelayProfileKind::Device => "device_development",
        _ => "unknown",
    }
}

#[cfg(feature = "mobile-social")]
const fn relay_aggregate_label(
    value: radroots_sdk::transport::RelayAggregateState,
) -> &'static str {
    match value {
        radroots_sdk::transport::RelayAggregateState::Configured => "configured",
        radroots_sdk::transport::RelayAggregateState::Connecting => "connecting",
        radroots_sdk::transport::RelayAggregateState::ReadOnly => "read_only",
        radroots_sdk::transport::RelayAggregateState::Writable => "writable",
        radroots_sdk::transport::RelayAggregateState::Degraded => "degraded",
        radroots_sdk::transport::RelayAggregateState::Offline => "offline",
        radroots_sdk::transport::RelayAggregateState::Failed => "failed",
        _ => "unknown",
    }
}

#[cfg(feature = "mobile-social")]
const fn transport_availability_label(
    value: radroots_transport::capability::Availability,
) -> &'static str {
    match value {
        radroots_transport::capability::Availability::Available => "available",
        radroots_transport::capability::Availability::Degraded => "degraded",
        radroots_transport::capability::Availability::Unavailable => "unavailable",
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
    async fn explicit_test_runtime_is_memory_backed() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
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
