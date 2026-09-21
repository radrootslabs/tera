use super::{coordinate_effect_support as effects, coordinate_support::*, *};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const AT: u64 = 1_750_000_000;
const DEADLINE: u64 = 2_000_000_000_000;

#[tokio::test]
async fn coordinate_clock_changes_at_effect_boundaries_preserve_one_preimage_and_saved_deadline() {
    for (rollback, before_sign) in [(true, false), (true, true), (false, false), (false, true)] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay = format!("ws://{}", listener.local_addr().unwrap());
        let signer = effects::PausedSigner::new();
        signer.pause.store(false, Ordering::SeqCst);
        let runtime = effects::runtime(signer.clone(), &relay);
        let saved = saved(&runtime, [211; 16], "clock:effects").await;
        let policy = Phase1QueuePolicy::new(
            vec![relay],
            Phase1RelaySatisfaction::AllAccepted,
            DEADLINE,
            Phase1CancellationPolicy::LocalCooperative,
        )
        .unwrap();
        let queued = runtime
            .phase1_queue_draft([211; 16], 1, policy, AT * 1_000 + 1)
            .await
            .unwrap();
        assert_eq!(
            queued.draft().created_at_unix_ms(),
            saved.draft().created_at_unix_ms()
        );
        let before = queued.draft().clone();
        let calls = AtomicUsize::new(0);
        let result = runtime
            .advance_owned_push_request_with_clock(push_request(&before).unwrap(), &before, || {
                let call = calls.fetch_add(1, Ordering::SeqCst);
                Ok(if call == 0 && !before_sign {
                    AT * 1_000 + 1
                } else if rollback {
                    (AT - 1) * 1_000
                } else {
                    DEADLINE + 1_000_000
                })
            })
            .await;
        if rollback {
            assert_eq!(result.unwrap_err(), Phase1DraftError::RevisionConflict);
        } else {
            result.unwrap();
        }
        let after = runtime.phase1_draft_status([211; 16]).await.unwrap();
        assert_eq!(after.draft(), &before);
        let push = after.push().unwrap();
        assert_eq!(push.delivery_plan().intent().deadline_unix_ms(), DEADLINE);
        assert!(push.delivery_plan().attempts().is_empty());
        assert_eq!(
            signer.calls.load(Ordering::SeqCst),
            usize::from(!before_sign)
        );
        assert_eq!(
            push.artifact().admission_state().is_admitted(),
            !rollback && !before_sign
        );
        match push.artifact().signed() {
            Some(signed) => assert_eq!(signed.event().envelope().created_at_u64(), AT),
            None => assert!(before_sign),
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
}
