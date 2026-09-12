use super::{Fault, FaultStore, Ordering};
use radroots_storage::{
    Error,
    atomic::AtomicCommitId,
    authored::{AuthoredArtifact, AuthoredArtifactId, AuthoredOperation},
    authored_atomic::{
        AuthoredAtomicCommand, AuthoredAtomicOutcome, AuthoredAtomicReceipt, AuthoredAtomicStorage,
    },
    authored_delivery::{AuthoredDeliveryPlan, AuthoredDeliveryPlanId},
    event::BoxFuture,
    journal::OperationInstanceId,
};

impl<S: AuthoredAtomicStorage + ?Sized> AuthoredAtomicStorage for FaultStore<'_, S> {
    fn execute_authored(
        &self,
        command: AuthoredAtomicCommand,
    ) -> BoxFuture<'_, Result<AuthoredAtomicReceipt, Error>> {
        Box::pin(async move {
            let attempt = self.commits.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 && matches!(self.atomic_fault, Fault::BeforeCommit) {
                return Err(Error::BackendUnavailable);
            }
            if let Fault::Race(barrier) = &self.atomic_fault {
                barrier.wait().await;
            }
            let receipt = self.inner.execute_authored(command).await?;
            if attempt == 0 {
                match self.atomic_fault {
                    Fault::LostCallback => return Err(Error::BackendUnavailable),
                    Fault::WrongReceipt => {
                        let AuthoredAtomicOutcome::Submitted(value) = receipt.outcome() else {
                            panic!("expected submission");
                        };
                        let prepared = value.preparation();
                        return AuthoredAtomicReceipt::from_durable_parts(
                            receipt.commit_id(),
                            receipt.digest(),
                            receipt.disposition(),
                            receipt.committed_at_unix_ms(),
                            AuthoredAtomicOutcome::Prepared {
                                operation: prepared.operation().clone(),
                                artifacts: prepared.artifacts().to_vec(),
                                delivery_plans: prepared.delivery_plans().to_vec(),
                            },
                        );
                    }
                    _ => {}
                }
            }
            Ok(receipt)
        })
    }
    fn authored_receipt(
        &self,
        id: AtomicCommitId,
    ) -> BoxFuture<'_, Result<Option<AuthoredAtomicReceipt>, Error>> {
        if let Some(value) = &self.receipt_override {
            return Box::pin(async { Ok(value.clone()) });
        }
        self.inner.authored_receipt(id)
    }
    fn authored_operation(
        &self,
        id: OperationInstanceId,
    ) -> BoxFuture<'_, Result<Option<AuthoredOperation>, Error>> {
        self.inner.authored_operation(id)
    }
    fn authored_artifact(
        &self,
        id: AuthoredArtifactId,
    ) -> BoxFuture<'_, Result<Option<AuthoredArtifact>, Error>> {
        self.inner.authored_artifact(id)
    }
    fn authored_delivery_plan(
        &self,
        id: AuthoredDeliveryPlanId,
    ) -> BoxFuture<'_, Result<Option<AuthoredDeliveryPlan>, Error>> {
        self.inner.authored_delivery_plan(id)
    }
}
