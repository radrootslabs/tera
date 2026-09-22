use radroots_storage::{
    Storage,
    authored_draft::{AuthoredDraft, AuthoredDraftStage},
};
use radroots_sync::{PullRequest, PushRequest, ingest::RegistryPolicy, pull::PullTermination};
use radroots_transport::{Target, source::FetchSelector, target::TargetSet};

use super::{
    ApplicationRestoreGuard, RestoreError as E,
    barrier::{self, Barrier, BarrierState},
    review_record::{RestoreObservation, RestoreTargetReview},
};
use crate::TeraRuntime;

impl TeraRuntime {
    /// One explicit bounded query of one original target and frozen publication.
    /// Current target observations never come from pre-backup local provenance.
    pub async fn reconcile_restored_target(
        &self,
        draft_id: [u8; 16],
        target_fingerprint: &str,
    ) -> Result<RestoreTargetReview, E> {
        let _maintenance = self.lifecycle.maintenance()?;
        let (guard, _, barrier) = self.restore_barrier().await?;
        if matches!(barrier.state, BarrierState::Resumed { .. }) {
            return Err(E::Conflict);
        }
        let inventory = self.restore_inventory().await?;
        let (_, request) = inventory
            .publications
            .iter()
            .find(|(key, _)| *key == draft_id)
            .ok_or(E::InvalidRequest)?;
        let target = request
            .targets()
            .targets()
            .iter()
            .find(|target| target.fingerprint().as_str() == target_fingerprint)
            .ok_or(E::InvalidRequest)?;
        let observation = self.observe_restored_target(request, target).await?;
        let now = now(&guard)?;
        let receipt = RestoreTargetReview::new(
            &guard,
            inventory.digest,
            draft_id,
            request,
            target,
            now,
            observation,
        )?;
        receipt
            .persist(self.client.storage().map_err(|_| E::Unavailable)?)
            .await?;
        Ok(receipt)
    }

    /// Records a review only after every pending original target has current
    /// evidence. Offline/partial attempts remain visible but cannot grant resume.
    pub async fn review_restored_work(&self) -> Result<[u8; 32], E> {
        let _maintenance = self.lifecycle.maintenance()?;
        let (guard, head, mut barrier) = self.restore_barrier().await?;
        if matches!(barrier.state, BarrierState::Resumed { .. }) {
            return Err(E::Conflict);
        }
        let digest = self.require_complete_restore_review(&guard).await?;
        barrier.state = BarrierState::Reviewed { inventory: digest };
        update(
            self.client.storage().map_err(|_| E::Unavailable)?,
            head,
            barrier,
            now(&guard)?,
        )
        .await?;
        Ok(digest)
    }

    /// Deliberate consent to resume the original work and identities after the
    /// displayed review. A changed inventory requires a new review. This never
    /// edits an original operation, creates a replacement event or sends it.
    pub async fn resume_restored_work(&self, reviewed_inventory: [u8; 32]) -> Result<(), E> {
        let _maintenance = self.lifecycle.maintenance()?;
        let (guard, head, mut barrier) = self.restore_barrier().await?;
        if let BarrierState::Resumed { inventory } = barrier.state {
            return if inventory == reviewed_inventory {
                Ok(())
            } else {
                Err(E::Conflict)
            };
        }
        if barrier.state
            != (BarrierState::Reviewed {
                inventory: reviewed_inventory,
            })
        {
            return Err(E::ReconciliationRequired);
        }
        if self.require_complete_restore_review(&guard).await? != reviewed_inventory {
            return Err(E::Conflict);
        }
        barrier.state = BarrierState::Resumed {
            inventory: reviewed_inventory,
        };
        update(
            self.client.storage().map_err(|_| E::Unavailable)?,
            head,
            barrier,
            now(&guard)?,
        )
        .await
    }

    async fn restore_barrier(
        &self,
    ) -> Result<(ApplicationRestoreGuard, AuthoredDraft, Barrier), E> {
        self.validate_restore_startup().await?;
        let guard = self.restore_guard.clone().ok_or(E::RecoveryRequired)?;
        let (head, barrier) = barrier::load(
            self.client.storage().map_err(|_| E::Unavailable)?,
            guard.request().backup().author(),
        )
        .await?
        .ok_or(E::RecoveryRequired)?;
        Ok((guard, head, barrier))
    }

    async fn require_complete_restore_review(
        &self,
        guard: &ApplicationRestoreGuard,
    ) -> Result<[u8; 32], E> {
        let inventory = self.restore_inventory().await?;
        let store = self.client.storage().map_err(|_| E::Unavailable)?;
        let now = now(guard)?;
        for (key, request) in &inventory.publications {
            for target in request.targets().targets() {
                let expected = RestoreTargetReview::new(
                    guard,
                    inventory.digest,
                    *key,
                    request,
                    target,
                    now,
                    RestoreObservation::Incomplete,
                )?;
                let receipt = expected
                    .load_current(store)
                    .await?
                    .ok_or(E::ReconciliationRequired)?;
                if receipt.observation == RestoreObservation::Incomplete
                    || receipt.observed_at_ms < guard.request().requested_at_ms()
                    || receipt.observed_at_ms > now
                {
                    return Err(E::ReconciliationRequired);
                }
            }
        }
        Ok(inventory.digest)
    }

    async fn observe_restored_target(
        &self,
        request: &PushRequest,
        target: &Target,
    ) -> Result<RestoreObservation, E> {
        let plan = request.plan();
        let selector = FetchSelector::all()
            .with_authors(vec![*plan.author()])
            .map_err(|_| E::VerificationFailed)?
            .with_kinds(vec![plan.body().kind()])
            .map_err(|_| E::VerificationFailed)?
            .with_since_unix_seconds(plan.created_at())
            .map_err(|_| E::VerificationFailed)?
            .with_until_unix_seconds(plan.created_at())
            .map_err(|_| E::VerificationFailed)?;
        let targets = TargetSet::new(vec![target.clone()]).map_err(|_| E::VerificationFailed)?;
        let pull = PullRequest::new(targets, 64, 2)
            .map_err(|_| E::InvalidRequest)?
            .with_selector(selector);
        let sync = self
            .client
            .sync()
            .map_err(|_| E::Unavailable)?
            .ok_or(E::Unavailable)?;
        let Ok(receipt) = sync.pull(pull, &RegistryPolicy::verified()).await else {
            return Ok(RestoreObservation::Incomplete);
        };
        if receipt
            .ingest_outcomes()
            .iter()
            .filter_map(|value| value.as_ref().ok())
            .any(|value| value.admission().event_id() == plan.expected_event_id())
        {
            return Ok(RestoreObservation::Observed);
        }
        let complete = receipt.termination() == PullTermination::Complete
            && receipt.ingest_outcomes().iter().all(Result::is_ok)
            && receipt.target_summaries().is_some_and(|summaries| {
                summaries.len() == 1
                    && summaries[0].target() == target.fingerprint()
                    && summaries[0].pages_observed() == receipt.pages_fetched()
                    && summaries[0].all_pages_complete()
            });
        Ok(if complete {
            RestoreObservation::NotObserved
        } else {
            RestoreObservation::Incomplete
        })
    }
}

fn now(guard: &ApplicationRestoreGuard) -> Result<u64, E> {
    let now = crate::runtime::product_surface::phase1_operation_now_unix_ms()
        .map_err(|_| E::Unavailable)?;
    if now < guard.request().requested_at_ms() || now > i64::MAX as u64 {
        return Err(E::InvalidRequest);
    }
    Ok(now)
}

async fn update(
    store: &dyn Storage,
    head: AuthoredDraft,
    barrier: Barrier,
    time: u64,
) -> Result<(), E> {
    let next = head
        .successor(barrier.encode()?, AuthoredDraftStage::Draft, None, time)
        .map_err(|_| E::Conflict)?;
    store
        .append_authored_draft(next, Some(head.revision()))
        .await
        .map_err(|_| E::Conflict)?;
    Ok(())
}
