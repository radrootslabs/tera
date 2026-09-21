use super::*;
use std::num::{NonZeroU32, NonZeroU64};

use nostr::prelude::{EventBuilder, JsonUtil, Keys, Timestamp};
use radroots_storage::{
    authored::{
        AuthoredArtifactId, FailureClass, RetrySchedule, WorkClaim, WorkFailure, WorkPhase,
    },
    authored_delivery::{AuthoredDeliveryAttempt, AuthoredDeliveryPlanId},
};
use radroots_transport::{
    DeliveryReceipt, DeliveryRequest, Target, TargetSet,
    outcome::DeliveryOutcome,
    policy::{SatisfactionClass, SatisfactionPolicy, SatisfactionState, TargetPolicy},
    sink::{DeliveryPayload, DeliveryTargetReceipt},
};

const NOW: u64 = 1_800_000_000_000;
const DAY: u64 = 24 * 60 * 60 * 1000;

fn plan(deadline: u64) -> AuthoredDeliveryPlan {
    let keys =
        Keys::parse("0000000000000000000000000000000000000000000000000000000000000001").unwrap();
    let event = EventBuilder::text_note("saved retry fixture")
        .custom_created_at(Timestamp::from_secs(NOW / 1000))
        .sign_with_keys(&keys)
        .unwrap();
    let event = radroots_event_codec::decode::signed_event(&event.as_json()).unwrap();
    let request = DeliveryRequest::new(
        "publication-retry-test",
        DeliveryPayload::new(event),
        TargetSet::new(vec![Target::nostr_relay("wss://relay.example").unwrap()]).unwrap(),
        SatisfactionPolicy::new(SatisfactionClass::Accepted, TargetPolicy::all()),
        deadline,
    )
    .unwrap();
    AuthoredDeliveryPlan::new_bound(
        AuthoredDeliveryPlanId::new([1; 16]).unwrap(),
        AuthoredArtifactId::new([2; 16]).unwrap(),
        request,
        NOW,
    )
    .unwrap()
}

fn receipt(plan: &AuthoredDeliveryPlan, outcome: DeliveryOutcome) -> DeliveryReceipt {
    let request = plan.request().unwrap();
    DeliveryReceipt::for_request(
        request,
        vec![DeliveryTargetReceipt::attempted(
            request.target_set().targets()[0].clone(),
            outcome,
        )],
    )
    .unwrap()
}

fn schedule(attempt: u32, at: u64) -> RetrySchedule {
    RetrySchedule::new(
        NonZeroU32::new(attempt).unwrap(),
        at,
        WorkFailure::new(
            "delivery_pending",
            WorkPhase::Delivery,
            FailureClass::Retryable,
            Some(at),
            None,
        )
        .unwrap(),
    )
    .unwrap()
}

fn apply(
    plan: &mut AuthoredDeliveryPlan,
    outcome: DeliveryOutcome,
    at: u64,
    retry: Option<RetrySchedule>,
) {
    let claim = WorkClaim::new(
        [3; 16],
        "retry-test",
        NonZeroU64::MIN,
        at,
        at + 10,
        plan.revision(),
    )
    .unwrap();
    let receipt = receipt(plan, outcome);
    plan.claim(claim.clone(), at).unwrap();
    plan.apply_receipt(
        claim.token(),
        claim.generation(),
        claim.row_revision(),
        receipt,
        retry,
        at + 1,
    )
    .unwrap();
}

#[test]
fn deadline_and_shared_admission_boundaries_never_erase_success_or_saved_input() {
    let policy = PublicationRetryPolicy::STANDARD;
    for window in [1_000, DAY] {
        let plan = plan(NOW + window);
        let original = plan.clone();
        let mut accepted = plan.clone();
        apply(&mut accepted, DeliveryOutcome::accepted(), NOW, None);
        assert_eq!(accepted.state(), AuthoredDeliveryState::Satisfied);
        assert_eq!(
            policy.decide(&plan, SyncRetryDecision::Ready, NOW + window - 1),
            PublicationRetryDecision::Ready
        );
        for at in [NOW + window, NOW + window + 1, u64::MAX] {
            assert_eq!(
                policy.decide(&plan, SyncRetryDecision::Expired, at),
                PublicationRetryDecision::NeedsAction(PublicationActionReason::DeadlineExceeded)
            );
            assert_eq!(
                policy.decide(&accepted, SyncRetryDecision::Satisfied, at),
                PublicationRetryDecision::Complete
            );
        }
        assert_eq!(plan, original);
        assert_eq!(
            policy.decide(
                &plan,
                SyncRetryDecision::InFlightUntil { unix_ms: NOW + 100 },
                NOW
            ),
            PublicationRetryDecision::InFlightUntil(NOW + 100)
        );
        assert_eq!(
            policy.decide(
                &plan,
                SyncRetryDecision::DeferredUntil { unix_ms: NOW + 200 },
                NOW
            ),
            PublicationRetryDecision::DeferredUntil(NOW + 200)
        );
        assert_eq!(
            policy.decide(
                &plan,
                SyncRetryDecision::DeferredUntil {
                    unix_ms: NOW + window
                },
                NOW
            ),
            PublicationRetryDecision::NeedsAction(PublicationActionReason::DeadlineExceeded)
        );
    }
}

#[test]
fn injected_backoff_is_bounded_spread_restart_stable_and_respects_retry_after() {
    let policy = PublicationRetryPolicy {
        base_delay_ms: 10,
        maximum_delay_ms: 80,
    };
    let delays: Vec<_> = (1..=100)
        .map(|attempt| policy.delay_ms(&[1; 16], attempt))
        .collect();
    assert!((10..=12).contains(&delays[0]));
    assert!((20..=25).contains(&delays[1]));
    assert!((40..=50).contains(&delays[2]));
    assert!(delays[3..].iter().all(|&delay| delay == 80));
    assert_eq!(
        delays,
        (1..=100)
            .map(|attempt| policy.delay_ms(&[1; 16], attempt))
            .collect::<Vec<_>>()
    );
    let spread: std::collections::BTreeSet<_> =
        (1..=32).map(|id| policy.delay_ms(&[id; 16], 3)).collect();
    assert!(spread.len() > 1);
    assert_eq!(policy.delay_ms(&[1; 16], u32::MAX), 80);
    let mut plan = plan(NOW + DAY);
    apply(
        &mut plan,
        DeliveryOutcome::unavailable(),
        NOW,
        Some(schedule(1, NOW + 200)),
    );
    let reloaded: AuthoredDeliveryPlan =
        serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
    assert_eq!(
        policy.decide(&plan, SyncRetryDecision::Ready, NOW + 100),
        PublicationRetryDecision::DeferredUntil(NOW + 200)
    );
    assert_eq!(
        policy.decide(&plan, SyncRetryDecision::Ready, NOW + 200),
        PublicationRetryDecision::Ready
    );
    assert_eq!(
        policy.decide(&reloaded, SyncRetryDecision::Ready, NOW + 100),
        policy.decide(&plan, SyncRetryDecision::Ready, NOW + 100)
    );
}

#[test]
fn real_cap_transition_retains_signed_payload_targets_and_all_attempts() {
    let mut plan = plan(NOW + DAY);
    let pending = receipt(&plan, DeliveryOutcome::unavailable());
    // Reconstruct a validated retained history at cap - 1, then exercise the
    // actual final claim/receipt transition. No threshold is reduced for tests.
    let count = DELIVERY_PLAN_ATTEMPTS_MAX - 1;
    let attempts: Vec<_> = (1..=count)
        .map(|attempt| {
            AuthoredDeliveryAttempt::reconstruct(
                NonZeroU32::new(attempt).unwrap(),
                NOW + 1,
                DeliveryAttemptOutcome::Receipt(pending.clone()),
                SatisfactionState::Pending,
            )
            .unwrap()
        })
        .collect();
    let retry = schedule(count, NOW + 2);
    let mut value = serde_json::to_value(&plan).unwrap();
    value["state"] = serde_json::json!("retryable");
    value["attempts"] = serde_json::to_value(attempts).unwrap();
    value["attempt_count"] = serde_json::json!(count);
    value["retry"] = serde_json::to_value(&retry).unwrap();
    value["last_failure"] = serde_json::to_value(retry.failure()).unwrap();
    value["updated_at_unix_ms"] = serde_json::json!(NOW + 1);
    plan = serde_json::from_value(value).unwrap();
    let saved_request = plan.request().unwrap().clone();
    assert_eq!(
        PublicationRetryPolicy::STANDARD.decide(&plan, SyncRetryDecision::Ready, NOW + 60_001),
        PublicationRetryDecision::Ready
    );
    apply(&mut plan, DeliveryOutcome::unavailable(), NOW + 2, None);
    assert_eq!(plan.state(), AuthoredDeliveryState::Exhausted);
    assert_eq!(plan.attempt_count(), DELIVERY_PLAN_ATTEMPTS_MAX);
    assert_eq!(plan.request(), Some(&saved_request));
    assert_eq!(
        PublicationRetryPolicy::STANDARD.decide(&plan, SyncRetryDecision::Exhausted, NOW + 3),
        PublicationRetryDecision::NeedsAction(PublicationActionReason::AttemptLimit)
    );
    let restored: AuthoredDeliveryPlan =
        serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
    assert_eq!(restored, plan);
    assert_eq!(restored.request(), Some(&saved_request));
}
