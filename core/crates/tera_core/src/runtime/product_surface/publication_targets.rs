//! Passive, bounded evidence for the frozen destinations of one publication.

use radroots_storage::{
    Storage,
    authored_delivery::DeliveryAttemptOutcome,
    event::{EventQuery, EventQueryBounds},
};
use radroots_sync::PushStatus;
use radroots_transport::{
    TransportId, outcome::DeliveryOutcomeKind, policy::SatisfactionClass,
    sink::DeliveryTargetReceipt,
};

/// Historical facts may coexist: a refusal does not erase an earlier acceptance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationTargetEvidence {
    pub fingerprint: String,
    pub endpoint: String,
    pub attempted: bool,
    pub accepted: bool,
    pub delivered: bool,
    pub rejected: bool,
    pub uncertain: bool,
    /// A retained inbound observation, never an OK or a retention guarantee.
    pub read_back_observed_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationTargetDetails {
    pub requires_delivery: bool,
    pub policy: PublicationTargetPolicy,
    pub targets: Vec<PublicationTargetEvidence>,
    pub read_back_available: bool,
    pub read_back_complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicationTargetPolicy {
    Any,
    All,
    Quorum(u16),
    Required(Vec<String>),
}

impl PublicationTargetDetails {
    pub(super) async fn load(push: &PushStatus, store: &dyn Storage) -> Self {
        let plan = push.delivery_plan();
        let unresolved = push.delivery_history().has_unresolved_claims();
        let policy = plan.intent().satisfaction().targets();
        let policy = if policy.is_any() {
            PublicationTargetPolicy::Any
        } else if policy.is_all() {
            PublicationTargetPolicy::All
        } else if let Some(threshold) = policy.quorum_threshold() {
            PublicationTargetPolicy::Quorum(threshold)
        } else {
            PublicationTargetPolicy::Required(
                policy
                    .required_targets()
                    .unwrap_or_default()
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
            )
        };
        let mut details = Self {
            requires_delivery: plan.intent().satisfaction().class() == SatisfactionClass::Delivered,
            policy,
            targets: plan
                .intent()
                .target_set()
                .targets()
                .iter()
                .map(|target| PublicationTargetEvidence {
                    fingerprint: target.fingerprint().as_str().to_owned(),
                    endpoint: target.uri().as_str().to_owned(),
                    attempted: false,
                    accepted: false,
                    delivered: false,
                    rejected: false,
                    uncertain: unresolved,
                    read_back_observed_at_unix_ms: None,
                })
                .collect(),
            read_back_available: true,
            read_back_complete: true,
        };
        for outcome in plan
            .attempts()
            .iter()
            .map(|attempt| attempt.outcome())
            .chain(plan.delivery_facts().iter().map(|fact| fact.outcome()))
        {
            details.observe_outcome(outcome);
        }
        if let Some(signed) = push.artifact().signed() {
            let query = EventQuery::for_ids(
                EventQueryBounds::first(1).expect("one event"),
                vec![*signed.event().id()],
            )
            .expect("one exact event ID");
            match store.query_raw(query).await {
                Ok(page) if page.items().is_empty() => return details,
                Ok(page)
                    if page.items().len() == 1
                        && page.items()[0].event().envelope() == signed.event().envelope() => {}
                _ => {
                    details.read_back_available = false;
                    details.read_back_complete = false;
                    return details;
                }
            }
            // A fixed page bounds status work independently of duplicate inbound traffic.
            // An incomplete/unavailable page never establishes absence of remote copies.
            match store
                .query_provenance(
                    *signed.event().id(),
                    EventQueryBounds::first(1_000).expect("fixed valid page bound"),
                )
                .await
            {
                Ok(page) => {
                    // Some stores cap provenance without a resumable cursor. A full page
                    // is therefore conservatively incomplete even when no cursor exists.
                    details.read_back_complete =
                        page.items().len() < 1_000 && page.next_cursor().is_none();
                    for item in page.items() {
                        let provenance = item.provenance();
                        if provenance.transport_id() != TransportId::NOSTR {
                            continue;
                        }
                        if let Some(target) = details
                            .targets
                            .iter_mut()
                            .find(|target| target.fingerprint == provenance.target().as_str())
                        {
                            // Retain a witnessed time, not a claim about the latest remote state.
                            target
                                .read_back_observed_at_unix_ms
                                .get_or_insert(provenance.observed_at_unix_ms());
                        }
                    }
                }
                Err(_) => {
                    details.read_back_available = false;
                    details.read_back_complete = false;
                }
            }
        }
        details
    }

    fn observe_outcome(&mut self, outcome: &DeliveryAttemptOutcome) {
        let receipts = match outcome {
            DeliveryAttemptOutcome::Receipt(receipt) => receipt.target_receipts(),
            DeliveryAttemptOutcome::SinkFailure(failure) => failure.partial_evidence(),
        };
        for target in &mut self.targets {
            if let Some(receipt) = receipts
                .iter()
                .find(|receipt| receipt.target().fingerprint().as_str() == target.fingerprint)
            {
                target.observe(receipt);
            } else {
                // A partial failure cannot prove that an omitted target was never attempted.
                target.uncertain = true;
            }
        }
    }
}

impl PublicationTargetEvidence {
    fn observe(&mut self, receipt: &DeliveryTargetReceipt) {
        self.attempted |= receipt.was_attempted();
        match receipt.outcome().kind() {
            DeliveryOutcomeKind::Accepted => self.accepted = true,
            DeliveryOutcomeKind::Delivered => {
                self.accepted = true;
                self.delivered = true;
            }
            DeliveryOutcomeKind::Rejected if receipt.was_attempted() => self.rejected = true,
            DeliveryOutcomeKind::Unavailable | DeliveryOutcomeKind::Failed => {
                self.uncertain |= receipt.was_attempted();
            }
            DeliveryOutcomeKind::Rejected => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radroots_transport::{Target, outcome::DeliveryOutcome};

    #[test]
    fn skipped_targets_and_duplicate_receipts_preserve_only_observed_facts() {
        let target = Target::nostr_relay("wss://relay.example").unwrap();
        let mut evidence = PublicationTargetEvidence {
            fingerprint: target.fingerprint().as_str().to_owned(),
            endpoint: target.uri().as_str().to_owned(),
            attempted: false,
            accepted: false,
            delivered: false,
            rejected: false,
            uncertain: false,
            read_back_observed_at_unix_ms: None,
        };
        evidence.observe(
            &DeliveryTargetReceipt::skipped(target.clone(), DeliveryOutcome::rejected()).unwrap(),
        );
        assert!(!evidence.attempted && !evidence.rejected && !evidence.accepted);
        evidence.observe(&DeliveryTargetReceipt::attempted(
            target.clone(),
            DeliveryOutcome::accepted(),
        ));
        let accepted = evidence.clone();
        evidence.observe(&DeliveryTargetReceipt::attempted(
            target.clone(),
            DeliveryOutcome::accepted(),
        ));
        assert!(evidence == accepted);
        evidence.observe(&DeliveryTargetReceipt::attempted(
            target.clone(),
            DeliveryOutcome::rejected(),
        ));
        evidence.observe(&DeliveryTargetReceipt::attempted(
            target,
            DeliveryOutcome::unavailable(),
        ));
        assert!(evidence.attempted && evidence.accepted && evidence.rejected && evidence.uncertain);
        assert!(!evidence.delivered && evidence.read_back_observed_at_unix_ms.is_none());
    }
}
