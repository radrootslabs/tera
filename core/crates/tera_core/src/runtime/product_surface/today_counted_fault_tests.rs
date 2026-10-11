//! Fault boundaries wrap actual provider calls; no operation returns a fabricated success.
use radroots_storage::Error;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Clone)]
pub(crate) struct FaultPause {
    pub(crate) entered: Arc<Notify>,
    pub(crate) resume: Arc<Notify>,
}
struct Armed {
    operation: &'static str,
    ordinal: usize,
    seen: usize,
    after: bool,
    refusal: Option<Error>,
    pause: Option<FaultPause>,
}
#[derive(Default)]
pub(crate) struct FaultController(Mutex<Option<Armed>>);
impl FaultController {
    pub(crate) fn failure(&self, operation: &'static str, ordinal: usize, after: bool) {
        self.refusal(operation, ordinal, after, Error::BackendUnavailable);
    }
    pub(crate) fn disk_full(&self, operation: &'static str, ordinal: usize, after: bool) {
        self.refusal(operation, ordinal, after, Error::SpaceInsufficient);
    }
    fn refusal(&self, operation: &'static str, ordinal: usize, after: bool, error: Error) {
        assert!(ordinal > 0);
        *self.0.lock().unwrap() = Some(Armed {
            operation,
            ordinal,
            seen: 0,
            after,
            refusal: Some(error),
            pause: None,
        });
    }
    pub(crate) fn pause(&self, operation: &'static str, ordinal: usize, after: bool) -> FaultPause {
        assert!(ordinal > 0);
        let pause = FaultPause {
            entered: Arc::new(Notify::new()),
            resume: Arc::new(Notify::new()),
        };
        *self.0.lock().unwrap() = Some(Armed {
            operation,
            ordinal,
            seen: 0,
            after,
            refusal: None,
            pause: Some(pause.clone()),
        });
        pause
    }
    pub(crate) async fn boundary(&self, operation: &'static str, after: bool) -> Result<(), Error> {
        let action = {
            let mut slot = self.0.lock().unwrap();
            if let Some(armed) = slot.as_mut()
                && armed.operation == operation
                && armed.after == after
            {
                armed.seen += 1;
                if armed.seen == armed.ordinal {
                    slot.take()
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(action) = action {
            if let Some(pause) = action.pause {
                pause.entered.notify_one();
                pause.resume.notified().await;
            }
            if let Some(error) = action.refusal {
                return Err(error);
            }
        }
        Ok(())
    }
}
