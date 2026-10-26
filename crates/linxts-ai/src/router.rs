use crate::protocol::CandidateEnvelope;
use crate::provider::{AiProvider, AiRequest, CancellationToken, InferenceBudget, ProviderError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Mini,
    Local3B,
    Local7B,
    LargerLocal,
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub offline: bool,
    pub online_authorized: bool,
    pub escalation_limit: usize,
}

impl Default for RoutePolicy {
    fn default() -> Self {
        Self {
            offline: false,
            online_authorized: false,
            escalation_limit: 4,
        }
    }
}

pub struct ProviderStage {
    pub tier: ModelTier,
    pub provider: Box<dyn AiProvider>,
}

impl std::fmt::Debug for ProviderStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderStage")
            .field("tier", &self.tier)
            .field("provider", &self.provider.id())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteAttempt {
    pub tier: ModelTier,
    pub provider_id: String,
    pub attempt: usize,
    pub outcome: String,
}

#[derive(Debug, Clone)]
pub struct RouteResult {
    pub candidate: CandidateEnvelope,
    pub attempts: Vec<RouteAttempt>,
}

#[derive(Debug, Default)]
pub struct AiRouter {
    stages: Vec<ProviderStage>,
}

impl AiRouter {
    #[must_use]
    pub fn new(stages: Vec<ProviderStage>) -> Self {
        Self { stages }
    }

    pub fn route(
        &self,
        request: &AiRequest,
        budget: &InferenceBudget,
        policy: RoutePolicy,
        cancellation: &CancellationToken,
    ) -> Result<RouteResult, RouteError> {
        request.validate(budget).map_err(RouteError::Request)?;
        cancellation.check().map_err(|_| RouteError::Cancelled)?;
        let mut attempts = Vec::new();
        let mut escalations = 0;

        for stage in &self.stages {
            cancellation.check().map_err(|_| RouteError::Cancelled)?;
            if stage.tier == ModelTier::Online && (policy.offline || !policy.online_authorized) {
                attempts.push(RouteAttempt {
                    tier: stage.tier,
                    provider_id: stage.provider.id().to_owned(),
                    attempt: 0,
                    outcome: if policy.offline {
                        "skipped: offline mode".to_owned()
                    } else {
                        "skipped: online provider lacks explicit authorization".to_owned()
                    },
                });
                continue;
            }
            if escalations >= policy.escalation_limit {
                break;
            }
            escalations += 1;

            match stage.provider.health(budget.timeout) {
                Ok(health) if health.available => {}
                Ok(health) => {
                    attempts.push(RouteAttempt {
                        tier: stage.tier,
                        provider_id: stage.provider.id().to_owned(),
                        attempt: 0,
                        outcome: format!("unavailable: {}", health.detail),
                    });
                    continue;
                }
                Err(error) => {
                    attempts.push(RouteAttempt {
                        tier: stage.tier,
                        provider_id: stage.provider.id().to_owned(),
                        attempt: 0,
                        outcome: format!("health check failed: {error}"),
                    });
                    continue;
                }
            }

            for attempt in 1..=budget.retries.saturating_add(1) {
                cancellation.check().map_err(|_| RouteError::Cancelled)?;
                match stage.provider.generate(request, budget, cancellation) {
                    Ok(mut candidate) => match candidate.validate(&request.source_hash) {
                        Ok(()) => {
                            if candidate.provider.is_none() {
                                candidate.set_provider("provider", stage.provider.id());
                            }
                            attempts.push(RouteAttempt {
                                tier: stage.tier,
                                provider_id: stage.provider.id().to_owned(),
                                attempt,
                                outcome: "valid candidate".to_owned(),
                            });
                            return Ok(RouteResult {
                                candidate,
                                attempts,
                            });
                        }
                        Err(error) => {
                            attempts.push(RouteAttempt {
                                tier: stage.tier,
                                provider_id: stage.provider.id().to_owned(),
                                attempt,
                                outcome: format!("invalid candidate: {error}"),
                            });
                            break;
                        }
                    },
                    Err(ProviderError::Cancelled) => return Err(RouteError::Cancelled),
                    Err(error) => {
                        let retryable = error.retryable();
                        attempts.push(RouteAttempt {
                            tier: stage.tier,
                            provider_id: stage.provider.id().to_owned(),
                            attempt,
                            outcome: error.to_string(),
                        });
                        if !retryable {
                            break;
                        }
                    }
                }
            }
        }

        Err(RouteError::Exhausted { attempts })
    }

    #[must_use]
    pub fn stages(&self) -> &[ProviderStage] {
        &self.stages
    }
}

#[derive(Debug, Error)]
pub enum RouteError {
    #[error("AI fallback was cancelled")]
    Cancelled,
    #[error("AI request is invalid: {0}")]
    Request(ProviderError),
    #[error("all permitted AI fallback stages were exhausted")]
    Exhausted { attempts: Vec<RouteAttempt> },
}

impl RouteError {
    #[must_use]
    pub fn attempts(&self) -> &[RouteAttempt] {
        match self {
            Self::Exhausted { attempts } => attempts,
            Self::Cancelled | Self::Request(_) => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        AI_SCHEMA_VERSION, CandidateLanguage, CandidateStatus, ProviderMetadata,
    };
    use crate::provider::ProviderHealth;
    use linxts_ir::{Expression, Module, Statement, StatementKind};
    use std::sync::Mutex;
    use std::time::Duration;

    struct MockProvider {
        id: String,
        candidate: Mutex<Option<CandidateEnvelope>>,
        available: bool,
    }

    impl AiProvider for MockProvider {
        fn id(&self) -> &str {
            &self.id
        }

        fn health(&self, _timeout: Duration) -> Result<ProviderHealth, ProviderError> {
            Ok(ProviderHealth {
                available: self.available,
                detail: "mock".to_owned(),
            })
        }

        fn generate(
            &self,
            _request: &AiRequest,
            _budget: &InferenceBudget,
            _cancellation: &CancellationToken,
        ) -> Result<CandidateEnvelope, ProviderError> {
            self.candidate
                .lock()
                .expect("lock")
                .take()
                .ok_or_else(|| ProviderError::Unavailable("no candidate".to_owned()))
        }
    }

    fn valid_candidate(hash: &str) -> CandidateEnvelope {
        let span = Default::default();
        let mut ir = Module::new(hash);
        ir.body.statements.push(Statement::new(
            StatementKind::Print(Expression::integer(1, span)),
            span,
        ));
        CandidateEnvelope {
            schema_version: AI_SCHEMA_VERSION.to_owned(),
            status: CandidateStatus::Candidate,
            language: CandidateLanguage::Natural,
            interpretation: "Print one".to_owned(),
            ir,
            assumptions: Vec::new(),
            warnings: Vec::new(),
            provider: Some(ProviderMetadata {
                kind: "mock".to_owned(),
                model_id: "mock".to_owned(),
            }),
        }
    }

    fn request() -> AiRequest {
        AiRequest {
            source: "print one".to_owned(),
            source_hash: "hash".to_owned(),
            language_hint: Some("natural".to_owned()),
            diagnostics: vec!["unresolved".to_owned()],
        }
    }

    #[test]
    fn escalates_from_unavailable_3b_to_valid_7b() {
        let router = AiRouter::new(vec![
            ProviderStage {
                tier: ModelTier::Local3B,
                provider: Box::new(MockProvider {
                    id: "3b".to_owned(),
                    candidate: Mutex::new(None),
                    available: false,
                }),
            },
            ProviderStage {
                tier: ModelTier::Local7B,
                provider: Box::new(MockProvider {
                    id: "7b".to_owned(),
                    candidate: Mutex::new(Some(valid_candidate("hash"))),
                    available: true,
                }),
            },
        ]);
        let result = router
            .route(
                &request(),
                &InferenceBudget::default(),
                RoutePolicy::default(),
                &CancellationToken::default(),
            )
            .expect("7B candidate");
        assert_eq!(result.attempts.len(), 2);
        assert_eq!(result.attempts[1].tier, ModelTier::Local7B);
    }

    #[test]
    fn never_uses_online_provider_without_authorization() {
        let router = AiRouter::new(vec![ProviderStage {
            tier: ModelTier::Online,
            provider: Box::new(MockProvider {
                id: "online".to_owned(),
                candidate: Mutex::new(Some(valid_candidate("hash"))),
                available: true,
            }),
        }]);
        let error = router
            .route(
                &request(),
                &InferenceBudget::default(),
                RoutePolicy::default(),
                &CancellationToken::default(),
            )
            .expect_err("authorization is required");
        assert!(error.attempts()[0].outcome.contains("authorization"));
    }

    #[test]
    fn cancellation_stops_before_inference() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let router = AiRouter::new(Vec::new());
        let error = router
            .route(
                &request(),
                &InferenceBudget::default(),
                RoutePolicy::default(),
                &cancellation,
            )
            .expect_err("cancelled");
        assert!(matches!(error, RouteError::Cancelled));
    }
}
