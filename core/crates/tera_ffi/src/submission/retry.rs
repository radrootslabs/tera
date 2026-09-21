use tera_core::runtime::product_surface::{PublicationActionReason, PublicationRetryDecision};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiPublicationActionReason {
    DeadlineExceeded,
    AttemptLimit,
    AuthenticationRequired,
    QuotaExceeded,
    InvalidPayload,
    DeliveryRefused,
    CoordinateChanged,
}

impl From<PublicationActionReason> for FfiPublicationActionReason {
    fn from(value: PublicationActionReason) -> Self {
        match value {
            PublicationActionReason::CoordinateChanged => Self::CoordinateChanged,
            PublicationActionReason::DeadlineExceeded => Self::DeadlineExceeded,
            PublicationActionReason::AttemptLimit => Self::AttemptLimit,
            PublicationActionReason::AuthenticationRequired => Self::AuthenticationRequired,
            PublicationActionReason::QuotaExceeded => Self::QuotaExceeded,
            PublicationActionReason::InvalidPayload => Self::InvalidPayload,
            PublicationActionReason::DeliveryRefused => Self::DeliveryRefused,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiPublicationRetryDecision {
    Ready,
    DeferredUntil { unix_ms: u64 },
    InFlightUntil { unix_ms: u64 },
    NeedsAction { reason: FfiPublicationActionReason },
    Complete,
    Stopped,
}

impl From<PublicationRetryDecision> for FfiPublicationRetryDecision {
    fn from(value: PublicationRetryDecision) -> Self {
        match value {
            PublicationRetryDecision::Ready => Self::Ready,
            PublicationRetryDecision::DeferredUntil(unix_ms) => Self::DeferredUntil { unix_ms },
            PublicationRetryDecision::InFlightUntil(unix_ms) => Self::InFlightUntil { unix_ms },
            PublicationRetryDecision::NeedsAction(reason) => Self::NeedsAction {
                reason: reason.into(),
            },
            PublicationRetryDecision::Complete => Self::Complete,
            PublicationRetryDecision::Stopped => Self::Stopped,
        }
    }
}
