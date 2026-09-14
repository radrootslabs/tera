//! Delivery observations remain independent of scheduling and stop intent.

use radroots_storage::authored_delivery::DeliveryAttemptOutcome;
use radroots_sync::PushStatus;
use radroots_transport::{outcome::DeliveryOutcomeKind, policy::SatisfactionState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationDeliveryState {
    /// The backend retains complete provenance proving no claim was issued.
    NotIssued,
    /// Issued or incomplete history cannot establish absence of remote effects.
    Unknown,
    PartiallyAccepted,
    /// Acceptance satisfies the frozen policy, not every possible destination.
    Accepted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationDeliveryEvidence {
    pub state: PublicationDeliveryState,
    pub stop_requested_at_unix_ms: Option<u64>,
    pub scheduling_revision: u64,
    pub retained_facts: u32,
    pub recorded_attempts: u32,
    pub unresolved_claims: bool,
}

impl PublicationDeliveryEvidence {
    pub fn from_push(push: &PushStatus) -> Self {
        let plan = push.delivery_plan();
        let history = push.delivery_history();
        let state = if plan.request().is_some()
            && plan.delivery_satisfaction() == Ok(SatisfactionState::Satisfied)
        {
            PublicationDeliveryState::Accepted
        } else if has_accepted_delivery(push) {
            PublicationDeliveryState::PartiallyAccepted
        } else if history.proves_no_issued_attempt() {
            PublicationDeliveryState::NotIssued
        } else {
            PublicationDeliveryState::Unknown
        };
        Self {
            state,
            stop_requested_at_unix_ms: plan.stop_requested_at_unix_ms(),
            scheduling_revision: plan.revision().get(),
            // Validated shared plans bound both collections to 1,024 entries.
            retained_facts: plan.delivery_facts().len() as u32,
            recorded_attempts: plan.attempt_count(),
            unresolved_claims: history.has_unresolved_claims(),
        }
    }
}

pub(super) fn has_accepted_delivery(push: &PushStatus) -> bool {
    let plan = push.delivery_plan();
    plan.attempts()
        .iter()
        .map(|attempt| attempt.outcome())
        .chain(plan.delivery_facts().iter().map(|fact| fact.outcome()))
        .any(|outcome| {
            let receipts = match outcome {
                DeliveryAttemptOutcome::Receipt(receipt) => receipt.target_receipts(),
                DeliveryAttemptOutcome::SinkFailure(failure) => failure.partial_evidence(),
            };
            receipts.iter().any(|receipt| {
                matches!(
                    receipt.outcome().kind(),
                    DeliveryOutcomeKind::Accepted | DeliveryOutcomeKind::Delivered
                )
            })
        })
}
