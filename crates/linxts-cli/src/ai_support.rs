use crate::CompileFlags;
use linxts_ai::{
    AiRequest, AiRouter, CancellationToken, InferenceBudget, LlamaCppProvider, ModelTier,
    ProviderStage, RoutePolicy,
};
use linxts_compiler::{Compilation, Diagnostic, Severity, Stage};
use linxts_source::SourceFile;
use std::time::Duration;

#[derive(Debug, Clone)]
struct EndpointConfig {
    tier: ModelTier,
    variable: &'static str,
    model_variable: &'static str,
    default_model: &'static str,
}

const ENDPOINTS: [EndpointConfig; 4] = [
    EndpointConfig {
        tier: ModelTier::Mini,
        variable: "LINXTS_LLAMA_MINI_URL",
        model_variable: "LINXTS_LLAMA_MINI_MODEL",
        default_model: "linxts-mini",
    },
    EndpointConfig {
        tier: ModelTier::Local3B,
        variable: "LINXTS_LLAMA_3B_URL",
        model_variable: "LINXTS_LLAMA_3B_MODEL",
        default_model: "local-3b",
    },
    EndpointConfig {
        tier: ModelTier::Local7B,
        variable: "LINXTS_LLAMA_7B_URL",
        model_variable: "LINXTS_LLAMA_7B_MODEL",
        default_model: "local-7b",
    },
    EndpointConfig {
        tier: ModelTier::LargerLocal,
        variable: "LINXTS_LLAMA_LARGE_URL",
        model_variable: "LINXTS_LLAMA_LARGE_MODEL",
        default_model: "local-larger",
    },
];

pub fn attempt_fallback(source: &SourceFile, compilation: &mut Compilation, flags: &CompileFlags) {
    if flags.no_ai || !eligible_for_ai(compilation) {
        return;
    }
    let (stages, configuration_errors) = configured_stages();
    if stages.is_empty() {
        if !configuration_errors.is_empty() {
            append_configuration_errors(compilation, configuration_errors);
        }
        return;
    }
    if !configuration_errors.is_empty() {
        append_configuration_errors(compilation, configuration_errors);
        return;
    }

    let request = AiRequest {
        source: source.text().to_owned(),
        source_hash: source.snapshot_hash().to_owned(),
        language_hint: Some(format!("{:?}", compilation.detection)),
        diagnostics: compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect(),
    };
    let budget = inference_budget();
    let router = AiRouter::new(stages);
    match router.route(
        &request,
        &budget,
        RoutePolicy {
            offline: flags.offline,
            online_authorized: false,
            escalation_limit: 4,
        },
        &CancellationToken::default(),
    ) {
        Ok(result) => {
            let provider = result
                .candidate
                .provider
                .as_ref()
                .map(|provider| provider.model_id.clone())
                .unwrap_or_else(|| "unknown local model".to_owned());
            compilation.module = result.candidate.ir;
            compilation.diagnostics.retain(|diagnostic| {
                !(diagnostic.severity == Severity::Error
                    && matches!(
                        diagnostic.stage,
                        Stage::Parser | Stage::Validation | Stage::Ai
                    ))
            });
            compilation.diagnostics.push(Diagnostic {
                code: "LXT-AI-200".to_owned(),
                severity: Severity::Info,
                stage: Stage::Ai,
                message: format!(
                    "accepted validated IR from {provider} after {} route attempt(s)",
                    result.attempts.len()
                ),
                message_zh_tw: Some("已接受本地模型產生且通過驗證的 IR".to_owned()),
                span: None,
                suggestion: None,
            });
            for attempt in &result.attempts {
                compilation.diagnostics.push(Diagnostic {
                    code: "LXT-AI-211".to_owned(),
                    severity: Severity::Info,
                    stage: Stage::Ai,
                    message: format!(
                        "{:?}/{}#{}: {}",
                        attempt.tier, attempt.provider_id, attempt.attempt, attempt.outcome
                    ),
                    message_zh_tw: Some("本地模型路由嘗試紀錄".to_owned()),
                    span: None,
                    suggestion: None,
                });
            }
            for warning in result.candidate.warnings {
                compilation.diagnostics.push(Diagnostic {
                    code: "LXT-AI-210".to_owned(),
                    severity: Severity::Warning,
                    stage: Stage::Ai,
                    message: warning,
                    message_zh_tw: Some("模型候選包含警告；請檢查解釋與 IR".to_owned()),
                    span: None,
                    suggestion: Some(
                        "Run `linxts explain` and verify the interpretation.".to_owned(),
                    ),
                });
            }
            compilation.used_ai = true;
        }
        Err(error) => {
            let route = error
                .attempts()
                .iter()
                .map(|attempt| {
                    format!(
                        "{:?}/{}#{}: {}",
                        attempt.tier, attempt.provider_id, attempt.attempt, attempt.outcome
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            compilation.diagnostics.push(Diagnostic {
                code: "LXT-AI-201".to_owned(),
                severity: Severity::Error,
                stage: Stage::Ai,
                message: if route.is_empty() {
                    error.to_string()
                } else {
                    format!("{error}. Attempts: {route}")
                },
                message_zh_tw: Some("所有允許的本地 AI 修復階段皆失敗".to_owned()),
                span: None,
                suggestion: Some(
                    "Check `linxts models test`, configure another local tier, or clarify the source."
                        .to_owned(),
                ),
            });
        }
    }
}

pub fn list_models() {
    let mut configured = 0;
    for endpoint in &ENDPOINTS {
        if std::env::var_os(endpoint.variable).is_some() {
            configured += 1;
            let model = std::env::var(endpoint.model_variable)
                .unwrap_or_else(|_| endpoint.default_model.to_owned());
            println!(
                "{:?}: configured ({}, model={model})",
                endpoint.tier, endpoint.variable
            );
        } else {
            println!(
                "{:?}: not configured ({})",
                endpoint.tier, endpoint.variable
            );
        }
    }
    if configured == 0 {
        println!("No local models configured; deterministic compilation remains available.");
    }
    println!("Online providers: not configured and never authorized implicitly.");
}

pub fn test_models() -> Result<(), ()> {
    let (stages, errors) = configured_stages();
    for error in &errors {
        eprintln!("[ERROR LXT-AI-202] {error}");
    }
    if stages.is_empty() {
        eprintln!("No valid llama.cpp endpoints are configured.");
        return Err(());
    }
    let mut failed = false;
    for stage in stages {
        match stage.provider.health(Duration::from_secs(3)) {
            Ok(health) if health.available => {
                println!(
                    "[OK] {:?}/{}: {}",
                    stage.tier,
                    stage.provider.id(),
                    health.detail
                );
            }
            Ok(health) => {
                failed = true;
                eprintln!(
                    "[ERROR] {:?}/{}: {}",
                    stage.tier,
                    stage.provider.id(),
                    health.detail
                );
            }
            Err(error) => {
                failed = true;
                eprintln!("[ERROR] {:?}/{}: {error}", stage.tier, stage.provider.id());
            }
        }
    }
    if failed || !errors.is_empty() {
        Err(())
    } else {
        Ok(())
    }
}

pub fn configured_count() -> usize {
    ENDPOINTS
        .iter()
        .filter(|endpoint| std::env::var_os(endpoint.variable).is_some())
        .count()
}

pub fn cache_model_identity() -> Option<String> {
    let models = ENDPOINTS
        .iter()
        .filter_map(|endpoint| {
            std::env::var_os(endpoint.variable).map(|_| {
                let model = std::env::var(endpoint.model_variable)
                    .unwrap_or_else(|_| endpoint.default_model.to_owned());
                format!("{:?}:{model}", endpoint.tier)
            })
        })
        .collect::<Vec<_>>();
    (!models.is_empty()).then(|| models.join("|"))
}

fn eligible_for_ai(compilation: &Compilation) -> bool {
    let errors: Vec<_> = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    !errors.is_empty()
        && errors
            .iter()
            .all(|diagnostic| matches!(diagnostic.stage, Stage::Parser | Stage::Validation))
}

fn configured_stages() -> (Vec<ProviderStage>, Vec<String>) {
    let mut stages = Vec::new();
    let mut errors = Vec::new();
    for endpoint in &ENDPOINTS {
        let Ok(url) = std::env::var(endpoint.variable) else {
            continue;
        };
        let model = std::env::var(endpoint.model_variable)
            .unwrap_or_else(|_| endpoint.default_model.to_owned());
        let id = format!("{:?}", endpoint.tier).to_lowercase();
        match LlamaCppProvider::new(id, model, &url) {
            Ok(provider) => stages.push(ProviderStage {
                tier: endpoint.tier,
                provider: Box::new(provider),
            }),
            Err(error) => errors.push(format!("{}: {error}", endpoint.variable)),
        }
    }
    (stages, errors)
}

fn inference_budget() -> InferenceBudget {
    let mut budget = InferenceBudget::default();
    if let Ok(seconds) = std::env::var("LINXTS_AI_TIMEOUT_SECS")
        && let Ok(seconds) = seconds.parse::<u64>()
    {
        budget.timeout = Duration::from_secs(seconds.clamp(1, 600));
    }
    if let Ok(retries) = std::env::var("LINXTS_AI_RETRIES")
        && let Ok(retries) = retries.parse::<usize>()
    {
        budget.retries = retries.min(5);
    }
    budget
}

fn append_configuration_errors(compilation: &mut Compilation, errors: Vec<String>) {
    compilation
        .diagnostics
        .extend(errors.into_iter().map(|message| Diagnostic {
            code: "LXT-AI-202".to_owned(),
            severity: Severity::Error,
            stage: Stage::Ai,
            message,
            message_zh_tw: Some("本地模型端點設定無效".to_owned()),
            span: None,
            suggestion: Some(
                "Use a loopback URL such as http://127.0.0.1:8080; remote endpoints require explicit API integration."
                    .to_owned(),
            ),
        }));
}
