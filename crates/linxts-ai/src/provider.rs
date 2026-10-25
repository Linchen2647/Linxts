use crate::protocol::{CandidateEnvelope, CandidateValidationError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceBudget {
    pub timeout: Duration,
    pub retries: usize,
    pub max_prompt_bytes: usize,
    pub max_response_bytes: usize,
    pub context_tokens: u32,
    pub output_tokens: u32,
}

impl Default for InferenceBudget {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            retries: 1,
            max_prompt_bytes: 256 * 1024,
            max_response_bytes: 4 * 1024 * 1024,
            context_tokens: 8192,
            output_tokens: 4096,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiRequest {
    pub source: String,
    pub source_hash: String,
    pub language_hint: Option<String>,
    pub diagnostics: Vec<String>,
}

impl AiRequest {
    pub fn validate(&self, budget: &InferenceBudget) -> Result<(), ProviderError> {
        if self.source.len() > budget.max_prompt_bytes {
            return Err(ProviderError::PromptTooLarge {
                actual: self.source.len(),
                limit: budget.max_prompt_bytes,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealth {
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), ProviderError> {
        if self.is_cancelled() {
            Err(ProviderError::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub trait AiProvider: Send + Sync {
    fn id(&self) -> &str;

    fn health(&self, timeout: Duration) -> Result<ProviderHealth, ProviderError>;

    fn generate(
        &self,
        request: &AiRequest,
        budget: &InferenceBudget,
        cancellation: &CancellationToken,
    ) -> Result<CandidateEnvelope, ProviderError>;
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider is unavailable: {0}")]
    Unavailable(String),
    #[error("provider timed out")]
    Timeout,
    #[error("inference was cancelled")]
    Cancelled,
    #[error("provider transport failed: {0}")]
    Transport(String),
    #[error("provider response was invalid JSON: {0}")]
    InvalidJson(String),
    #[error("provider response exceeded {limit} bytes")]
    ResponseTooLarge { limit: usize },
    #[error("prompt is {actual} bytes; budget permits {limit}")]
    PromptTooLarge { actual: usize, limit: usize },
    #[error("AI candidate failed validation: {0}")]
    InvalidCandidate(#[from] CandidateValidationError),
    #[error("provider returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
}

impl ProviderError {
    #[must_use]
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::Unavailable(_)
                | Self::Transport(_)
                | Self::Http {
                    status: 429 | 500..=599,
                    ..
                }
        )
    }
}
