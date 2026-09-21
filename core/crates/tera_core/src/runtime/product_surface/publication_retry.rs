//! Product retry admission over the existing durable delivery plan.

use radroots_protocol::runtime::v1::SyncRetryDecision;
use radroots_storage::authored_delivery::{
    AuthoredDeliveryPlan, AuthoredDeliveryState, DELIVERY_PLAN_ATTEMPTS_MAX, DeliveryAttemptOutcome,
};
use radroots_sync::PushStatus;
use sha2::{Digest, Sha256};

use super::Phase1DraftError;
use crate::TeraRuntime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationActionReason {
    DeadlineExceeded,
    AttemptLimit,
    AuthenticationRequired,
    QuotaExceeded,
    InvalidPayload,
    DeliveryRefused,
    CoordinateChanged,
}

/// Permission to start another attempt, separate from any retained remote facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationRetryDecision {
    Ready,
    DeferredUntil(u64),
    InFlightUntil(u64),
    NeedsAction(PublicationActionReason),
    Complete,
    Stopped,
}

impl PublicationRetryDecision {
    pub const fn may_start(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Injected into the pure decision. No task, clock read or persisted copy of policy.
#[derive(Clone, Copy)]
pub(super) struct PublicationRetryPolicy {
    base_delay_ms: u64,
    maximum_delay_ms: u64,
}

impl PublicationRetryPolicy {
    pub(super) const STANDARD: Self = Self {
        base_delay_ms: 1_000,
        maximum_delay_ms: 60_000,
    };

    pub(super) fn decide(
        self,
        plan: &AuthoredDeliveryPlan,
        shared: SyncRetryDecision,
        now_unix_ms: u64,
    ) -> PublicationRetryDecision {
        use PublicationActionReason as Reason;
        use PublicationRetryDecision as Decision;
        // Shared policy includes claim-bound late facts. Scheduling exhaustion
        // or a later clock value cannot erase a satisfied frozen target policy.
        if shared == SyncRetryDecision::Satisfied {
            return Decision::Complete;
        }
        if plan.stop_requested_at_unix_ms().is_some()
            || plan.state() == AuthoredDeliveryState::Cancelled
        {
            return Decision::Stopped;
        }
        if let Some(reason) = action_reason(plan) {
            return Decision::NeedsAction(reason);
        }
        if plan.attempt_count() >= DELIVERY_PLAN_ATTEMPTS_MAX {
            return Decision::NeedsAction(Reason::AttemptLimit);
        }
        if now_unix_ms >= plan.intent().deadline_unix_ms() {
            return Decision::NeedsAction(Reason::DeadlineExceeded);
        }
        let shared_not_before = match shared {
            SyncRetryDecision::Ready => 0,
            SyncRetryDecision::DeferredUntil { unix_ms } => unix_ms,
            SyncRetryDecision::InFlightUntil { unix_ms } => {
                return Decision::InFlightUntil(unix_ms);
            }
            SyncRetryDecision::Expired => {
                return Decision::NeedsAction(Reason::DeadlineExceeded);
            }
            SyncRetryDecision::Exhausted => {
                return Decision::NeedsAction(Reason::DeliveryRefused);
            }
            SyncRetryDecision::Satisfied => return Decision::Complete,
        };
        let product_not_before = plan.attempts().last().map_or(0, |attempt| {
            attempt
                .recorded_at_unix_ms()
                .saturating_add(self.delay_ms(plan.plan_id().as_bytes(), plan.attempt_count()))
        });
        let not_before = shared_not_before
            .max(plan.retry().map_or(0, |retry| retry.not_before_unix_ms()))
            .max(product_not_before);
        if not_before >= plan.intent().deadline_unix_ms() {
            return Decision::NeedsAction(Reason::DeadlineExceeded);
        }
        if now_unix_ms < not_before {
            Decision::DeferredUntil(not_before)
        } else {
            Decision::Ready
        }
    }

    fn delay_ms(self, identity: &[u8; 16], attempt: u32) -> u64 {
        let exponent = attempt.saturating_sub(1).min(63);
        let base = self
            .base_delay_ms
            .saturating_mul(1u64 << exponent)
            .min(self.maximum_delay_ms);
        // The original plan identity and attempt number make jitter stable
        // across status reads and restart. It is scheduling spread, not entropy
        // for a secret, token or signing operation.
        let mut hash = Sha256::new();
        hash.update(b"tera.publication-retry.v1\0");
        hash.update(identity);
        hash.update(attempt.to_be_bytes());
        let hash = hash.finalize();
        let sample = u64::from_be_bytes(hash[..8].try_into().expect("fixed SHA-256 prefix"));
        base.saturating_add(sample % (base / 4 + 1))
            .min(self.maximum_delay_ms)
    }
}

fn reason_for_code(code: &str) -> Option<PublicationActionReason> {
    use PublicationActionReason as Reason;
    match code {
        "auth_required" => Some(Reason::AuthenticationRequired),
        "quota_exceeded" => Some(Reason::QuotaExceeded),
        "malformed_event" => Some(Reason::InvalidPayload),
        "rejected" => Some(Reason::DeliveryRefused),
        "delivery_deadline_exceeded" => Some(Reason::DeadlineExceeded),
        "delivery_attempt_limit" => Some(Reason::AttemptLimit),
        _ => None,
    }
}

fn action_reason(plan: &AuthoredDeliveryPlan) -> Option<PublicationActionReason> {
    // Retained refusal is not authorization to retry an unchanged payload.
    // A newly satisfied frozen policy was handled before this inspection.
    for outcome in plan
        .delivery_facts()
        .iter()
        .map(|fact| fact.outcome())
        .chain(plan.attempts().iter().map(|attempt| attempt.outcome()))
    {
        let receipts = match outcome {
            DeliveryAttemptOutcome::Receipt(receipt) => receipt.target_receipts(),
            DeliveryAttemptOutcome::SinkFailure(failure) => {
                if let Some(reason) = reason_for_code(failure.code()) {
                    return Some(reason);
                }
                failure.partial_evidence()
            }
        };
        if let Some(reason) = receipts
            .iter()
            .filter_map(|receipt| receipt.outcome().code().and_then(reason_for_code))
            .next()
        {
            return Some(reason);
        }
    }
    plan.last_failure()
        .and_then(|failure| reason_for_code(failure.code()))
}

impl TeraRuntime {
    pub(super) fn publication_retry_at(
        &self,
        push: &PushStatus,
        now_unix_ms: u64,
    ) -> Result<PublicationRetryDecision, Phase1DraftError> {
        if now_unix_ms == 0 {
            return Err(Phase1DraftError::Operation);
        }
        let plan = push.delivery_plan();
        let now = now_unix_ms.max(plan.updated_at_unix_ms());
        let shared = self
            .sync()?
            .retry_decision(plan, now)
            .map_err(|_| Phase1DraftError::Operation)?;
        Ok(PublicationRetryPolicy::STANDARD.decide(plan, shared, now))
    }
}

#[cfg(test)]
mod tests;
