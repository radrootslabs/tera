use super::{
    RestoreError as E, RestoreObservation, RestoreTargetReview,
    barrier::{self, BarrierState},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePhase {
    Held,
    Reviewed,
    Resumed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestorePendingTarget {
    pub draft_id: [u8; 16],
    pub event_id: [u8; 32],
    pub target_fingerprint: String,
    pub observation: Option<RestoreObservation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreStatus {
    pub attempt_id: [u8; 16],
    pub phase: RestorePhase,
    pub targets: Vec<RestorePendingTarget>,
}

impl crate::TeraRuntime {
    pub async fn restore_status(&self) -> Result<Option<RestoreStatus>, E> {
        let _maintenance = self.lifecycle.maintenance()?;
        self.validate_restore_startup().await?;
        let Some(guard) = &self.restore_guard else {
            return Ok(None);
        };
        let store = self.client.storage().map_err(|_| E::Unavailable)?;
        let (_, barrier) = barrier::load(store, guard.request().backup().author())
            .await?
            .ok_or(E::RecoveryRequired)?;
        let phase = match barrier.state {
            BarrierState::Held => RestorePhase::Held,
            BarrierState::Reviewed { .. } => RestorePhase::Reviewed,
            BarrierState::Resumed { .. } => RestorePhase::Resumed,
        };
        let mut targets = Vec::new();
        if phase != RestorePhase::Resumed {
            let inventory = self.restore_inventory().await?;
            for (key, request) in inventory.publications {
                for target in request.targets().targets() {
                    let expected = RestoreTargetReview::new(
                        guard,
                        inventory.digest,
                        key,
                        &request,
                        target,
                        guard.request().requested_at_ms(),
                        RestoreObservation::Incomplete,
                    )?;
                    let receipt = expected.load_current(store).await?;
                    targets.push(RestorePendingTarget {
                        draft_id: key,
                        event_id: expected.event_id,
                        target_fingerprint: expected.target_fingerprint,
                        observation: receipt.map(|r| r.observation),
                    });
                }
            }
        }
        Ok(Some(RestoreStatus {
            attempt_id: guard.request().attempt_id(),
            phase,
            targets,
        }))
    }
}
