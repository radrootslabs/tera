use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum RadrootsAppError {
    #[error("initialization: {0}")]
    Initialization(String),
    #[error("identity: {0}")]
    Identity(String),
    #[error("secure store: {0}")]
    SecureStore(String),
    #[error("relay: {0}")]
    Relay(String),
    #[error("runtime: {0}")]
    Runtime(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl RadrootsAppError {
    pub fn initialization(message: impl Into<String>) -> Self {
        Self::Initialization(message.into())
    }

    pub fn identity(message: impl Into<String>) -> Self {
        Self::Identity(message.into())
    }

    pub fn secure_store(message: impl Into<String>) -> Self {
        Self::SecureStore(message.into())
    }

    pub fn relay(message: impl Into<String>) -> Self {
        Self::Relay(message.into())
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}
