use std::error::Error as StdError;
use thiserror::Error;

pub type AudioResult<T> = Result<T, AudioError>;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("{message}")]
    Message { message: String },
    #[error("buffer '{name}' not found in registry")]
    BufferNotFound { name: String },
    #[error("invalid flow index: {index} (max flow={max})")]
    InvalidFlowIndex { index: usize, max: usize },
    #[error("invalid producer index: {index} (max producer={max})")]
    InvalidProducerIndex { index: usize, max: usize },
    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl AudioError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message {
            message: message.into(),
        }
    }

pub fn with_context(
    context: impl Into<String>,
    source: impl Into<anyhow::Error>,
) -> Self {
    AudioError::Context {
        context: context.into(),
        source: source.into().into(),
    }
}
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{message}")]
    Message { message: String },
    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl ConfigError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message {
            message: message.into(),
        }
    }

    pub fn with_context<E>(context: impl Into<String>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Context {
            context: context.into(),
            source: Box::new(source),
        }
    }
}
