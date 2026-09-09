//! Caller-owned command leases and one resumable close attempt.
//!
//! Closing admission is permanent. No lock crosses an await, no task or runtime
//! is created here, and dropping a lease never edits durable operation evidence.

use std::{
    future::poll_fn,
    sync::Mutex,
    task::{Poll, Waker},
};

use radroots_protocol::error::v1::{KnownCode, SCHEMA_VERSION};

use crate::{SdkErrorRecord, TeraAppError};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuntimeLifecycleError {
    #[error("runtime close requires completion or retry")]
    Closing,
    #[error("runtime is closed")]
    Closed,
    #[error("runtime close is in progress")]
    CloseInProgress,
    #[error("runtime lifecycle is unavailable")]
    Unavailable,
}

impl RuntimeLifecycleError {
    pub const fn code(self) -> &'static str {
        self.known_code().as_str()
    }

    const fn known_code(self) -> KnownCode {
        match self {
            Self::Closing => KnownCode::ClientClosing,
            Self::Closed => KnownCode::ClientClosed,
            Self::CloseInProgress => KnownCode::ClientCloseInProgress,
            Self::Unavailable => KnownCode::InternalError,
        }
    }
}

impl From<RuntimeLifecycleError> for TeraAppError {
    fn from(error: RuntimeLifecycleError) -> Self {
        let descriptor = error.known_code().descriptor();
        let sdk = match error {
            RuntimeLifecycleError::Closing => Some(radroots_sdk::error::ErrorKind::ClientClosing),
            RuntimeLifecycleError::Closed => Some(radroots_sdk::error::ErrorKind::ClientClosed),
            RuntimeLifecycleError::CloseInProgress => {
                Some(radroots_sdk::error::ErrorKind::CloseInProgress)
            }
            RuntimeLifecycleError::Unavailable => None,
        }
        .map(radroots_sdk::error::ErrorKind::descriptor);
        Self::Sdk {
            report: SdkErrorRecord {
                schema_version: SCHEMA_VERSION,
                code: error.code().to_owned(),
                class: descriptor.class.as_str().to_owned(),
                retryable: descriptor.retryable,
                recovery_actions: descriptor
                    .recovery_actions
                    .iter()
                    .map(|action| action.as_str().to_owned())
                    .collect(),
                operation_id: None,
                capability_id: sdk
                    .and_then(|value| value.capability())
                    .map(|value| value.as_str().to_owned()),
                message: sdk.map_or_else(|| error.to_string(), |value| value.message().to_owned()),
            },
        }
    }
}

#[derive(Default)]
struct State {
    closing: bool,
    active: usize,
    close_active: bool,
    completed: Option<Result<(), SdkErrorRecord>>,
    close_waker: Option<Waker>,
}

#[derive(Default)]
pub(super) struct RuntimeLifecycle {
    state: Mutex<State>,
}

impl RuntimeLifecycle {
    pub(super) fn enter(&self) -> Result<CommandLease<'_>, RuntimeLifecycleError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RuntimeLifecycleError::Unavailable)?;
        if state.closing {
            return Err(if state.completed.is_some() {
                RuntimeLifecycleError::Closed
            } else {
                RuntimeLifecycleError::Closing
            });
        }
        state.active = state
            .active
            .checked_add(1)
            .ok_or(RuntimeLifecycleError::Unavailable)?;
        Ok(CommandLease(self))
    }

    pub(super) fn begin_close(&self) -> Result<CloseAttempt<'_>, RuntimeLifecycleError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RuntimeLifecycleError::Unavailable)?;
        state.closing = true;
        if state.close_active {
            return Err(RuntimeLifecycleError::CloseInProgress);
        }
        state.close_active = true;
        Ok(CloseAttempt(self))
    }
}

pub(super) struct CommandLease<'a>(&'a RuntimeLifecycle);

impl Drop for CommandLease<'_> {
    fn drop(&mut self) {
        let wake = if let Ok(mut state) = self.0.state.lock() {
            state.active -= 1;
            if state.active == 0 {
                state.close_waker.take()
            } else {
                None
            }
        } else {
            None
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

pub(super) struct CloseAttempt<'a>(&'a RuntimeLifecycle);

impl CloseAttempt<'_> {
    pub(super) async fn drain(&self) -> Result<(), RuntimeLifecycleError> {
        poll_fn(|context| {
            let Ok(mut state) = self.0.state.lock() else {
                return Poll::Ready(Err(RuntimeLifecycleError::Unavailable));
            };
            if state.active == 0 {
                Poll::Ready(Ok(()))
            } else {
                state.close_waker = Some(context.waker().clone());
                Poll::Pending
            }
        })
        .await
    }

    pub(super) fn completed(
        &self,
    ) -> Result<Option<Result<(), SdkErrorRecord>>, RuntimeLifecycleError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| RuntimeLifecycleError::Unavailable)?;
        Ok(state.completed.clone())
    }

    pub(super) fn complete(
        &self,
        result: Result<(), SdkErrorRecord>,
    ) -> Result<(), RuntimeLifecycleError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| RuntimeLifecycleError::Unavailable)?;
        state.completed = Some(result);
        Ok(())
    }
}

impl Drop for CloseAttempt<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.close_active = false;
            state.close_waker = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completed_close_failure_is_replayed_without_false_success() {
        let runtime = crate::TeraRuntime::test_memory().unwrap();
        let report = SdkErrorRecord {
            schema_version: 1,
            code: "storage_close_failed".to_owned(),
            class: "storage".to_owned(),
            retryable: false,
            recovery_actions: vec!["inspect_local_stores".to_owned()],
            operation_id: None,
            capability_id: Some("storage.canonical".to_owned()),
            message: "SDK storage close failed".to_owned(),
        };
        // Inject the exact terminal SDK report at the close commit point.
        // This exercises repeat-close behavior without changing the producer.
        runtime
            .lifecycle
            .begin_close()
            .unwrap()
            .complete(Err(report.clone()))
            .unwrap();
        for _ in 0..2 {
            let TeraAppError::Sdk { report: replay } = runtime.shutdown().await.unwrap_err() else {
                panic!("stable close report");
            };
            assert_eq!(replay, report);
            assert!(runtime.sdk_storage_status().await.is_err());
        }
    }
}
