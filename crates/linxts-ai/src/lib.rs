//! Validated AI fallback protocol and local llama.cpp escalation router.
//!
//! No model is downloaded or contacted unless a caller explicitly configures
//! a provider and invokes the router.

mod llama_cpp;
mod protocol;
mod provider;
mod router;

pub use llama_cpp::{HttpEndpoint, LlamaCppProvider};
pub use protocol::{
    AI_SCHEMA_VERSION, CandidateEnvelope, CandidateLanguage, CandidateStatus,
    CandidateValidationError, ProviderMetadata, candidate_json_schema,
};
pub use provider::{
    AiProvider, AiRequest, CancellationToken, InferenceBudget, ProviderError, ProviderHealth,
};
pub use router::{
    AiRouter, ModelTier, ProviderStage, RouteAttempt, RouteError, RoutePolicy, RouteResult,
};
