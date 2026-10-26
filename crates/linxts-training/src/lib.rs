//! Reproducible dataset generation and evaluation for Linxts-Mini.

use linxts_ai::{AI_SCHEMA_VERSION, CandidateEnvelope, CandidateLanguage, CandidateStatus};
use linxts_compiler::{CompileOptions, Compiler, Detection, Language};
use linxts_ir::{IR_SCHEMA_VERSION, Module, Validator};
use linxts_runtime::Vm;
use linxts_source::{SourceFile, SourceId, Span};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const DATASET_SCHEMA_VERSION: &str = "linxts-mini-dataset-1";
pub const DEFAULT_DATASET_SEED: u64 = 20_261_026;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingTask {
    LanguageDetection,
    SyntaxRepair,
    NaturalLanguageToIr,
    MixedLanguageUnderstanding,
    StructuredIrGeneration,
    InvalidIrCorrection,
}

impl TrainingTask {
    fn as_str(self) -> &'static str {
        match self {
            Self::LanguageDetection => "language_detection",
            Self::SyntaxRepair => "syntax_repair",
            Self::NaturalLanguageToIr => "natural_language_to_ir",
            Self::MixedLanguageUnderstanding => "mixed_language_understanding",
            Self::StructuredIrGeneration => "structured_ir_generation",
            Self::InvalidIrCorrection => "invalid_ir_correction",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetSplit {
    Train,
    Validation,
    Test,
}

impl DatasetSplit {
    fn file_name(self) -> &'static str {
        match self {
            Self::Train => "train.jsonl",
            Self::Validation => "validation.jsonl",
            Self::Test => "test.jsonl",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainingInput {
    pub source: String,
    pub diagnostics: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_ir: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleVerification {
    pub source_hash: String,
    pub expected_stdout: String,
    pub label_source: String,
    pub input_compiles_without_ai: bool,
    pub compiler_validated: bool,
    pub ir_validator_passed: bool,
    pub execution_checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleProvenance {
    pub source_path: String,
    pub source_reference: String,
    pub generator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainingSample {
    pub schema_version: String,
    pub id: String,
    pub group_id: String,
    pub split: DatasetSplit,
    pub task: TrainingTask,
    pub input: TrainingInput,
    pub target: CandidateEnvelope,
    pub verification: SampleVerification,
    pub provenance: SampleProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitManifest {
    pub samples: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetManifest {
    pub schema_version: String,
    pub generator_version: String,
    pub ir_schema_version: String,
    pub ai_schema_version: String,
    pub seed: u64,
    pub total_samples: usize,
    pub task_counts: BTreeMap<String, usize>,
    pub splits: BTreeMap<String, SplitManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationReport {
    pub total_samples: usize,
    pub valid_samples: usize,
    pub duplicate_samples: usize,
    pub split_leaks: usize,
    pub errors: Vec<String>,
}

impl ValidationReport {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.errors.is_empty()
            && self.total_samples == self.valid_samples
            && self.duplicate_samples == 0
            && self.split_leaks == 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredictionRecord {
    pub sample_id: String,
    pub candidate: Value,
    #[serde(default)]
    pub latency_ms: Option<f64>,
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkReport {
    pub system: String,
    pub total_samples: usize,
    pub answered_samples: usize,
    pub language_detection_accuracy: f64,
    pub repair_parse_success_rate: f64,
    pub ir_schema_validity: f64,
    pub ir_validator_pass_rate: f64,
    pub execution_correctness: f64,
    pub average_latency_ms: Option<f64>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub escalation_required_rate: f64,
    pub notes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum TrainingError {
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("output directory is not empty: {0}")]
    ExistingOutput(PathBuf),
    #[error("dataset sample is invalid: {0}")]
    InvalidSample(String),
    #[error("deterministic compiler rejected label source for {id}: {diagnostics:?}")]
    LabelCompilation {
        id: String,
        diagnostics: Vec<String>,
    },
    #[error("VM execution failed for {id}: {message}")]
    Execution { id: String, message: String },
}

#[derive(Clone)]
struct SeedSpec {
    id: &'static str,
    split: DatasetSplit,
    task: TrainingTask,
    language: CandidateLanguage,
    source: SeedSource,
    label_source: Option<&'static str>,
    expected_stdout: &'static str,
    provenance_path: &'static str,
    provenance_reference: &'static str,
}

#[derive(Clone)]
enum SeedSource {
    Inline(&'static str),
    RepositoryFile(&'static str),
}

#[derive(Clone)]
struct GeneratedSpec {
    id: String,
    split: DatasetSplit,
    task: TrainingTask,
    language: CandidateLanguage,
    source: String,
    label_source: String,
    expected_stdout: String,
    provenance_reference: String,
}

struct SampleDefinition<'a> {
    id: &'a str,
    split: DatasetSplit,
    task: TrainingTask,
    language: CandidateLanguage,
    expected_stdout: &'a str,
    provenance_path: &'a str,
    provenance_reference: &'a str,
}

#[must_use]
pub fn seed_count() -> usize {
    seed_specs().len() + generated_specs(DEFAULT_DATASET_SEED).len()
}

pub fn generate_dataset(
    repository_root: &Path,
    output_directory: &Path,
    seed: u64,
) -> Result<DatasetManifest, TrainingError> {
    refuse_nonempty_directory(output_directory)?;
    fs::create_dir_all(output_directory)?;

    let mut samples = Vec::new();
    let mut duplicate_keys = HashSet::new();
    for specification in seed_specs() {
        let source = match specification.source {
            SeedSource::Inline(source) => source.to_owned(),
            SeedSource::RepositoryFile(relative) => {
                fs::read_to_string(repository_root.join(relative))?
            }
        };
        let label_source = specification.label_source.unwrap_or(&source).to_owned();
        let definition = SampleDefinition {
            id: specification.id,
            split: specification.split,
            task: specification.task,
            language: specification.language,
            expected_stdout: specification.expected_stdout,
            provenance_path: specification.provenance_path,
            provenance_reference: specification.provenance_reference,
        };
        let sample = build_sample(&definition, source, label_source)?;
        let duplicate_key = sample_duplicate_key(&sample)?;
        if !duplicate_keys.insert(duplicate_key) {
            return Err(TrainingError::InvalidSample(format!(
                "duplicate generated sample {}",
                sample.id
            )));
        }
        samples.push(sample);
    }
    for specification in generated_specs(seed) {
        let definition = SampleDefinition {
            id: &specification.id,
            split: specification.split,
            task: specification.task,
            language: specification.language,
            expected_stdout: &specification.expected_stdout,
            provenance_path: "crates/linxts-training/src/lib.rs",
            provenance_reference: &specification.provenance_reference,
        };
        let sample = build_sample(
            &definition,
            specification.source,
            specification.label_source,
        )?;
        let duplicate_key = sample_duplicate_key(&sample)?;
        if !duplicate_keys.insert(duplicate_key) {
            return Err(TrainingError::InvalidSample(format!(
                "duplicate generated sample {}",
                sample.id
            )));
        }
        samples.push(sample);
    }
    samples.sort_by(|left, right| left.id.cmp(&right.id));

    let mut split_bytes = BTreeMap::new();
    for split in [
        DatasetSplit::Train,
        DatasetSplit::Validation,
        DatasetSplit::Test,
    ] {
        let selected: Vec<_> = samples
            .iter()
            .filter(|sample| sample.split == split)
            .collect();
        let mut bytes = Vec::new();
        for sample in &selected {
            serde_json::to_writer(&mut bytes, sample)?;
            bytes.push(b'\n');
        }
        fs::write(output_directory.join(split.file_name()), &bytes)?;
        split_bytes.insert(split, (selected.len(), bytes));
    }

    let mut task_counts = BTreeMap::new();
    for sample in &samples {
        *task_counts
            .entry(sample.task.as_str().to_owned())
            .or_insert(0) += 1;
    }
    let splits = split_bytes
        .into_iter()
        .map(|(split, (count, bytes))| {
            (
                split.file_name().trim_end_matches(".jsonl").to_owned(),
                SplitManifest {
                    samples: count,
                    sha256: sha256_hex(&bytes),
                },
            )
        })
        .collect();
    let manifest = DatasetManifest {
        schema_version: DATASET_SCHEMA_VERSION.to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        ir_schema_version: IR_SCHEMA_VERSION.to_owned(),
        ai_schema_version: AI_SCHEMA_VERSION.to_owned(),
        seed,
        total_samples: samples.len(),
        task_counts,
        splits,
    };
    fs::write(
        output_directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    let report = validate_dataset(output_directory)?;
    if !report.passed() {
        return Err(TrainingError::InvalidSample(format!(
            "generated dataset failed validation: {:?}",
            report.errors
        )));
    }
    Ok(manifest)
}

pub fn validate_dataset(directory: &Path) -> Result<ValidationReport, TrainingError> {
    let mut report = ValidationReport {
        total_samples: 0,
        valid_samples: 0,
        duplicate_samples: 0,
        split_leaks: 0,
        errors: Vec::new(),
    };
    let mut duplicate_keys = HashSet::new();
    let mut groups = HashMap::<String, DatasetSplit>::new();

    for expected_split in [
        DatasetSplit::Train,
        DatasetSplit::Validation,
        DatasetSplit::Test,
    ] {
        let path = directory.join(expected_split.file_name());
        for (index, line) in read_json_lines(&path)?.into_iter().enumerate() {
            report.total_samples += 1;
            let sample = match serde_json::from_str::<TrainingSample>(&line) {
                Ok(sample) => sample,
                Err(error) => {
                    report.errors.push(format!(
                        "{}:{} cannot decode sample: {error}",
                        path.display(),
                        index + 1
                    ));
                    continue;
                }
            };
            let mut valid = true;
            if sample.split != expected_split {
                report
                    .errors
                    .push(format!("{} is stored in the wrong split file", sample.id));
                valid = false;
            }
            if let Some(previous) = groups.insert(sample.group_id.clone(), sample.split)
                && previous != sample.split
            {
                report.split_leaks += 1;
                report.errors.push(format!(
                    "group {} appears in {:?} and {:?}",
                    sample.group_id, previous, sample.split
                ));
                valid = false;
            }
            match sample_duplicate_key(&sample) {
                Ok(key) => {
                    if !duplicate_keys.insert(key) {
                        report.duplicate_samples += 1;
                        report
                            .errors
                            .push(format!("duplicate sample content: {}", sample.id));
                        valid = false;
                    }
                }
                Err(error) => {
                    report.errors.push(error.to_string());
                    valid = false;
                }
            }
            if let Err(error) = validate_sample(&sample) {
                report.errors.push(format!("{}: {error}", sample.id));
                valid = false;
            }
            if valid {
                report.valid_samples += 1;
            }
        }
    }
    validate_manifest(directory, &mut report)?;
    Ok(report)
}

pub fn benchmark_no_ai(directory: &Path) -> Result<BenchmarkReport, TrainingError> {
    benchmark_no_ai_split(directory, None)
}

pub fn benchmark_no_ai_split(
    directory: &Path,
    split: Option<DatasetSplit>,
) -> Result<BenchmarkReport, TrainingError> {
    let samples = load_samples(directory, split)?;
    let mut metrics = Metrics::default();
    let mut elapsed = Duration::ZERO;
    for sample in &samples {
        let source = SourceFile::from_text(SourceId(0), &sample.id, &sample.input.source);
        let start = Instant::now();
        let compilation = Compiler::compile(
            &source,
            CompileOptions {
                no_ai: true,
                ..CompileOptions::default()
            },
        );
        elapsed += start.elapsed();
        metrics.total += 1;
        if language_for_detection(&compilation.detection) == Some(sample.target.language) {
            metrics.language_correct += 1;
        }
        if !compilation.has_errors() {
            metrics.answered += 1;
            metrics.schema_valid += 1;
            if Validator::validate(&compilation.module).is_empty() {
                metrics.validator_valid += 1;
            }
            if execute_module(&sample.id, &compilation.module)?
                == sample.verification.expected_stdout
            {
                metrics.execution_correct += 1;
            }
        }
        if sample.task == TrainingTask::SyntaxRepair {
            metrics.repair_total += 1;
            if !compilation.has_errors() && !compilation.repairs.is_empty() {
                metrics.repair_success += 1;
            }
        }
    }
    let average = if metrics.total == 0 {
        None
    } else {
        Some(elapsed.as_secs_f64() * 1000.0 / metrics.total as f64)
    };
    Ok(metrics.report(
        "linxts-no-ai",
        average,
        None,
        None,
        vec![
            "Latency measures deterministic compilation plus validation on this host.".to_owned(),
            "Token counts are unavailable because no model tokenizer is involved.".to_owned(),
        ],
    ))
}

pub fn benchmark_predictions(
    directory: &Path,
    predictions_path: &Path,
    system: &str,
) -> Result<BenchmarkReport, TrainingError> {
    benchmark_predictions_split(directory, predictions_path, system, None)
}

pub fn benchmark_predictions_split(
    directory: &Path,
    predictions_path: &Path,
    system: &str,
    split: Option<DatasetSplit>,
) -> Result<BenchmarkReport, TrainingError> {
    let samples = load_samples(directory, split)?;
    let predictions: HashMap<_, _> = read_json_lines(predictions_path)?
        .into_iter()
        .map(|line| serde_json::from_str::<PredictionRecord>(&line))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|record| (record.sample_id.clone(), record))
        .collect();
    let mut metrics = Metrics::default();
    let mut latency_sum = 0.0;
    let mut latency_count = 0_u64;
    let mut prompt_tokens = 0_u64;
    let mut completion_tokens = 0_u64;
    let mut has_token_counts = false;

    for sample in &samples {
        metrics.total += 1;
        if sample.task == TrainingTask::SyntaxRepair {
            metrics.repair_total += 1;
        }
        let Some(prediction) = predictions.get(&sample.id) else {
            continue;
        };
        metrics.answered += 1;
        if let Some(latency) = prediction.latency_ms {
            latency_sum += latency;
            latency_count += 1;
        }
        if let (Some(prompt), Some(completion)) =
            (prediction.prompt_tokens, prediction.completion_tokens)
        {
            prompt_tokens += prompt;
            completion_tokens += completion;
            has_token_counts = true;
        }
        let Ok(candidate) =
            serde_json::from_value::<CandidateEnvelope>(prediction.candidate.clone())
        else {
            continue;
        };
        metrics.schema_valid += 1;
        if candidate.language == sample.target.language {
            metrics.language_correct += 1;
        }
        if candidate
            .validate(&sample.verification.source_hash)
            .is_err()
        {
            continue;
        }
        metrics.validator_valid += 1;
        let output = execute_module(&sample.id, &candidate.ir)?;
        if output == sample.verification.expected_stdout {
            metrics.execution_correct += 1;
            if sample.task == TrainingTask::SyntaxRepair {
                metrics.repair_success += 1;
            }
        }
    }
    Ok(metrics.report(
        system,
        (latency_count > 0).then_some(latency_sum / latency_count as f64),
        has_token_counts.then_some(prompt_tokens),
        has_token_counts.then_some(completion_tokens),
        vec![
            "Schema validity requires strict CandidateEnvelope deserialization.".to_owned(),
            "Execution correctness compares VM stdout with the fixed held-out oracle.".to_owned(),
        ],
    ))
}

fn build_sample(
    specification: &SampleDefinition<'_>,
    source: String,
    label_source: String,
) -> Result<TrainingSample, TrainingError> {
    let input_file = SourceFile::from_text(SourceId(0), specification.id, &source);
    let input_compilation = Compiler::compile(
        &input_file,
        CompileOptions {
            no_ai: true,
            ..CompileOptions::default()
        },
    );
    let label_file = SourceFile::from_text(SourceId(0), "label.lx", &label_source);
    let label_compilation = Compiler::compile(
        &label_file,
        CompileOptions {
            no_ai: true,
            ..CompileOptions::default()
        },
    );
    if label_compilation.has_errors() {
        return Err(TrainingError::LabelCompilation {
            id: specification.id.to_owned(),
            diagnostics: label_compilation
                .diagnostics
                .iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                .collect(),
        });
    }
    let source_hash = input_file.snapshot_hash().to_owned();
    let module = rebind_module(label_compilation.module, &source_hash, source.len())?;
    if !Validator::validate(&module).is_empty() {
        return Err(TrainingError::InvalidSample(format!(
            "{} label failed IR validation",
            specification.id
        )));
    }
    let output = execute_module(specification.id, &module)?;
    if output != specification.expected_stdout {
        return Err(TrainingError::InvalidSample(format!(
            "{} expected stdout {:?}, got {:?}",
            specification.id, specification.expected_stdout, output
        )));
    }

    let invalid_ir = if specification.task == TrainingTask::InvalidIrCorrection {
        let mut invalid = serde_json::to_value(&module)?;
        invalid["schema_version"] = Value::String("invalid-training-fixture".to_owned());
        let decoded: Module = serde_json::from_value(invalid.clone())?;
        if Validator::validate(&decoded).is_empty() {
            return Err(TrainingError::InvalidSample(format!(
                "{} invalid IR mutation was accepted",
                specification.id
            )));
        }
        Some(invalid)
    } else {
        None
    };
    let diagnostics = input_compilation
        .diagnostics
        .iter()
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect();
    let assumptions = input_compilation
        .repairs
        .iter()
        .map(|repair| format!("{}: {}", repair.id, repair.reason))
        .collect();
    let target = CandidateEnvelope {
        schema_version: AI_SCHEMA_VERSION.to_owned(),
        status: CandidateStatus::Candidate,
        language: specification.language,
        interpretation: format!(
            "Validated Linxts IR label for {} task",
            specification.task.as_str()
        ),
        ir: module,
        assumptions,
        warnings: Vec::new(),
        provider: None,
    };
    target.validate(&source_hash).map_err(|error| {
        TrainingError::InvalidSample(format!(
            "{} candidate failed validation: {error}",
            specification.id
        ))
    })?;

    let fingerprint = sha256_hex(
        format!(
            "{}\0{}\0{}\0{}",
            DATASET_SCHEMA_VERSION,
            specification.id,
            specification.task.as_str(),
            source
        )
        .as_bytes(),
    );
    Ok(TrainingSample {
        schema_version: DATASET_SCHEMA_VERSION.to_owned(),
        id: format!("{}-{}", specification.id, &fingerprint[..12]),
        group_id: specification.id.to_owned(),
        split: specification.split,
        task: specification.task,
        input: TrainingInput {
            source,
            diagnostics,
            invalid_ir,
        },
        target,
        verification: SampleVerification {
            source_hash,
            expected_stdout: specification.expected_stdout.to_owned(),
            label_source,
            input_compiles_without_ai: !input_compilation.has_errors(),
            compiler_validated: true,
            ir_validator_passed: true,
            execution_checked: true,
        },
        provenance: SampleProvenance {
            source_path: specification.provenance_path.to_owned(),
            source_reference: specification.provenance_reference.to_owned(),
            generator: "linxts-training deterministic seed generator".to_owned(),
        },
    })
}

fn validate_sample(sample: &TrainingSample) -> Result<(), TrainingError> {
    if sample.schema_version != DATASET_SCHEMA_VERSION {
        return Err(TrainingError::InvalidSample(format!(
            "unsupported dataset schema {}",
            sample.schema_version
        )));
    }
    let source = SourceFile::from_text(SourceId(0), &sample.id, &sample.input.source);
    if source.snapshot_hash() != sample.verification.source_hash {
        return Err(TrainingError::InvalidSample(
            "source hash does not match sample input".to_owned(),
        ));
    }
    sample
        .target
        .validate(source.snapshot_hash())
        .map_err(|error| TrainingError::InvalidSample(error.to_string()))?;

    let label_file =
        SourceFile::from_text(SourceId(0), "label.lx", &sample.verification.label_source);
    let compilation = Compiler::compile(
        &label_file,
        CompileOptions {
            no_ai: true,
            ..CompileOptions::default()
        },
    );
    if compilation.has_errors() {
        return Err(TrainingError::InvalidSample(
            "stored label source no longer compiles".to_owned(),
        ));
    }
    let expected = rebind_module(
        compilation.module,
        &sample.verification.source_hash,
        sample.input.source.len(),
    )?;
    if expected != sample.target.ir {
        return Err(TrainingError::InvalidSample(
            "target IR differs from current deterministic compiler output".to_owned(),
        ));
    }
    let output = execute_module(&sample.id, &sample.target.ir)?;
    if output != sample.verification.expected_stdout {
        return Err(TrainingError::InvalidSample(format!(
            "expected stdout {:?}, got {:?}",
            sample.verification.expected_stdout, output
        )));
    }
    if sample.task == TrainingTask::InvalidIrCorrection {
        let invalid = sample.input.invalid_ir.clone().ok_or_else(|| {
            TrainingError::InvalidSample("invalid-IR task lacks invalid_ir input".to_owned())
        })?;
        let invalid: Module = serde_json::from_value(invalid)?;
        if Validator::validate(&invalid).is_empty() {
            return Err(TrainingError::InvalidSample(
                "invalid_ir input unexpectedly passes validation".to_owned(),
            ));
        }
    } else if sample.input.invalid_ir.is_some() {
        return Err(TrainingError::InvalidSample(
            "non-correction task contains invalid_ir input".to_owned(),
        ));
    }
    Ok(())
}

fn validate_manifest(directory: &Path, report: &mut ValidationReport) -> Result<(), TrainingError> {
    let manifest: DatasetManifest =
        serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
    if manifest.schema_version != DATASET_SCHEMA_VERSION {
        report
            .errors
            .push("manifest dataset schema mismatch".to_owned());
    }
    if manifest.ir_schema_version != IR_SCHEMA_VERSION {
        report.errors.push("manifest IR schema mismatch".to_owned());
    }
    if manifest.total_samples != report.total_samples {
        report.errors.push(format!(
            "manifest says {} samples but files contain {}",
            manifest.total_samples, report.total_samples
        ));
    }
    for split in [
        DatasetSplit::Train,
        DatasetSplit::Validation,
        DatasetSplit::Test,
    ] {
        let name = split.file_name().trim_end_matches(".jsonl");
        let bytes = fs::read(directory.join(split.file_name()))?;
        let Some(expected) = manifest.splits.get(name) else {
            report.errors.push(format!("manifest lacks {name} split"));
            continue;
        };
        if expected.sha256 != sha256_hex(&bytes) {
            report
                .errors
                .push(format!("{name} split fingerprint mismatch"));
        }
    }
    Ok(())
}

fn load_samples(
    directory: &Path,
    selected_split: Option<DatasetSplit>,
) -> Result<Vec<TrainingSample>, TrainingError> {
    let mut samples = Vec::new();
    for split in [
        DatasetSplit::Train,
        DatasetSplit::Validation,
        DatasetSplit::Test,
    ] {
        if selected_split.is_some_and(|selected| selected != split) {
            continue;
        }
        for line in read_json_lines(&directory.join(split.file_name()))? {
            samples.push(serde_json::from_str(&line)?);
        }
    }
    Ok(samples)
}

fn read_json_lines(path: &Path) -> Result<Vec<String>, TrainingError> {
    let reader = BufReader::new(fs::File::open(path)?);
    reader
        .lines()
        .filter_map(|line| match line {
            Ok(line) if line.trim().is_empty() => None,
            other => Some(other.map_err(TrainingError::Io)),
        })
        .collect()
}

fn execute_module(id: &str, module: &Module) -> Result<String, TrainingError> {
    let mut output = Vec::new();
    Vm::new(&mut output)
        .execute(module)
        .map_err(|error| TrainingError::Execution {
            id: id.to_owned(),
            message: error.to_string(),
        })?;
    String::from_utf8(output).map_err(|error| TrainingError::Execution {
        id: id.to_owned(),
        message: error.to_string(),
    })
}

fn rebind_module(
    module: Module,
    source_hash: &str,
    source_len: usize,
) -> Result<Module, TrainingError> {
    let mut value = serde_json::to_value(module)?;
    value["source_hash"] = Value::String(source_hash.to_owned());
    let span = serde_json::to_value(Span::new(SourceId(0), 0, source_len))?;
    replace_spans(&mut value, &span);
    Ok(serde_json::from_value(value)?)
}

fn replace_spans(value: &mut Value, span: &Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("span") {
                object.insert("span".to_owned(), span.clone());
            }
            for child in object.values_mut() {
                replace_spans(child, span);
            }
        }
        Value::Array(values) => {
            for child in values {
                replace_spans(child, span);
            }
        }
        _ => {}
    }
}

fn sample_duplicate_key(sample: &TrainingSample) -> Result<String, TrainingError> {
    let invalid = sample
        .input
        .invalid_ir
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?
        .unwrap_or_default();
    Ok(sha256_hex(
        format!(
            "{}\0{}\0{}",
            sample.task.as_str(),
            sample.input.source,
            invalid
        )
        .as_bytes(),
    ))
}

fn refuse_nonempty_directory(path: &Path) -> Result<(), TrainingError> {
    if path.is_dir() && fs::read_dir(path)?.next().is_some() {
        return Err(TrainingError::ExistingOutput(path.to_owned()));
    }
    if path.exists() && !path.is_dir() {
        return Err(TrainingError::ExistingOutput(path.to_owned()));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn language_for_detection(detection: &Detection) -> Option<CandidateLanguage> {
    match detection {
        Detection::Mixed { .. } => Some(CandidateLanguage::Mixed),
        _ => detection.primary_language().map(candidate_language),
    }
}

fn candidate_language(language: Language) -> CandidateLanguage {
    match language {
        Language::Python => CandidateLanguage::Python,
        Language::JavaScript => CandidateLanguage::Javascript,
        Language::Rust => CandidateLanguage::Rust,
        Language::C => CandidateLanguage::C,
        Language::Natural => CandidateLanguage::Natural,
        Language::Generic => CandidateLanguage::Generic,
    }
}

#[derive(Default)]
struct Metrics {
    total: usize,
    answered: usize,
    language_correct: usize,
    repair_total: usize,
    repair_success: usize,
    schema_valid: usize,
    validator_valid: usize,
    execution_correct: usize,
}

impl Metrics {
    fn report(
        &self,
        system: &str,
        average_latency_ms: Option<f64>,
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        notes: Vec<String>,
    ) -> BenchmarkReport {
        BenchmarkReport {
            system: system.to_owned(),
            total_samples: self.total,
            answered_samples: self.answered,
            language_detection_accuracy: ratio(self.language_correct, self.total),
            repair_parse_success_rate: ratio(self.repair_success, self.repair_total),
            ir_schema_validity: ratio(self.schema_valid, self.total),
            ir_validator_pass_rate: ratio(self.validator_valid, self.total),
            execution_correctness: ratio(self.execution_correct, self.total),
            average_latency_ms,
            prompt_tokens,
            completion_tokens,
            escalation_required_rate: 1.0 - ratio(self.execution_correct, self.total),
            notes,
        }
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn generated_specs(seed: u64) -> Vec<GeneratedSpec> {
    use CandidateLanguage as L;
    use TrainingTask as T;

    let mut specifications = Vec::new();
    let mut add = |family: &str,
                   value: i64,
                   task: TrainingTask,
                   language: CandidateLanguage,
                   source: String,
                   expected_stdout: String| {
        let id = format!("generated-{family}-{value}");
        specifications.push(GeneratedSpec {
            split: generated_split(&id, seed),
            provenance_reference: format!(
                "parameterized {family} family generated with fixed seed {seed}"
            ),
            label_source: source.clone(),
            id,
            task,
            language,
            source,
            expected_stdout,
        });
    };

    for value in 100_i64..200 {
        add(
            "generic-print",
            value,
            T::LanguageDetection,
            L::Generic,
            format!("PRINT {value}\n"),
            format!("{value}\n"),
        );
        add(
            "python-format",
            value,
            T::StructuredIrGeneration,
            L::Python,
            format!("^python\nx = {value}\nprint(f\"值={{x}}\")\n"),
            format!("值={value}\n"),
        );
        add(
            "javascript-template",
            value,
            T::StructuredIrGeneration,
            L::Javascript,
            format!("^javascript\nlet x = {value}; console.log(`值=${{x}}`);\n"),
            format!("值={value}\n"),
        );
        add(
            "rust-format",
            value,
            T::StructuredIrGeneration,
            L::Rust,
            format!("^rust\nlet x = {value}; println!(\"值={{}}\", x);\n"),
            format!("值={value}\n"),
        );
        add(
            "c-format",
            value,
            T::StructuredIrGeneration,
            L::C,
            format!("^c\nint x = {value}; printf(\"值=%d\", x);\n"),
            format!("值={value}\n"),
        );
        add(
            "javascript-repair",
            value,
            T::SyntaxRepair,
            L::Javascript,
            format!("^javascript\nconsole.log(\"repair-{value}\"\n"),
            format!("repair-{value}\n"),
        );
        add(
            "python-repair",
            value,
            T::SyntaxRepair,
            L::Python,
            format!("^python\nprint(\"修復{value}\n"),
            format!("修復{value}\n"),
        );
        add(
            "invalid-ir",
            value,
            T::InvalidIrCorrection,
            L::Generic,
            format!("x = {value}\nprint(x + 1)\n"),
            format!("{}\n", value + 1),
        );
        let sum = value * (value + 1) / 2;
        add(
            "natural-sum",
            value,
            T::NaturalLanguageToIr,
            L::Natural,
            format!("^自然語言\n請計算1到{value}的總和\n"),
            format!("{sum}\n"),
        );
        add(
            "python-range",
            value,
            T::StructuredIrGeneration,
            L::Python,
            format!(
                "^python\ntotal = 0\nfor i in range(1, {}):\n    total = total + i\nprint(total)\n",
                value + 1
            ),
            format!("{sum}\n"),
        );
        add(
            "mixed-shared",
            value,
            T::MixedLanguageUnderstanding,
            L::Mixed,
            format!(
                "^python\nx = {value}\n^javascript\nconsole.log(x)\n^自然語言\n把 x 乘以 2，然後顯示結果\n"
            ),
            format!("{value}\n{}\n", value * 2),
        );
    }
    specifications
}

fn generated_split(group_id: &str, seed: u64) -> DatasetSplit {
    let digest = Sha256::digest(format!("{seed}:{group_id}").as_bytes());
    if digest[0] < 26 {
        DatasetSplit::Validation
    } else {
        DatasetSplit::Train
    }
}

fn seed_specs() -> Vec<SeedSpec> {
    use CandidateLanguage as L;
    use DatasetSplit as S;
    use TrainingTask as T;
    vec![
        SeedSpec {
            id: "generic-print",
            split: S::Train,
            task: T::LanguageDetection,
            language: L::Generic,
            source: SeedSource::RepositoryFile("examples/hello.lx"),
            label_source: None,
            expected_stdout: "1\n",
            provenance_path: "examples/hello.lx",
            provenance_reference: "generic PRINT example",
        },
        SeedSpec {
            id: "javascript-missing-paren",
            split: S::Train,
            task: T::SyntaxRepair,
            language: L::Javascript,
            source: SeedSource::Inline("^javascript\nconsole.log(\"Hello\"\n"),
            label_source: None,
            expected_stdout: "Hello\n",
            provenance_path: "crates/linxts-compiler/src/repair.rs",
            provenance_reference: "repairs_javascript_parenthesis",
        },
        SeedSpec {
            id: "rust-missing-paren",
            split: S::Train,
            task: T::SyntaxRepair,
            language: L::Rust,
            source: SeedSource::Inline("^rust\nprintln!(\"repaired Rust\"\n"),
            label_source: None,
            expected_stdout: "repaired Rust\n",
            provenance_path: "examples/broken-syntax.lx",
            provenance_reference: "missing delimiter pattern",
        },
        SeedSpec {
            id: "natural-sum-zh-tw",
            split: S::Train,
            task: T::NaturalLanguageToIr,
            language: L::Natural,
            source: SeedSource::RepositoryFile("examples/natural-language.lx"),
            label_source: None,
            expected_stdout: "5050\n",
            provenance_path: "examples/natural-language.lx",
            provenance_reference: "Traditional Chinese deterministic sum",
        },
        SeedSpec {
            id: "mixed-shared-variable",
            split: S::Train,
            task: T::MixedLanguageUnderstanding,
            language: L::Mixed,
            source: SeedSource::RepositoryFile("examples/mixed.lx"),
            label_source: None,
            expected_stdout: "10\n50\n",
            provenance_path: "examples/mixed.lx",
            provenance_reference: "cross-language shared variable",
        },
        SeedSpec {
            id: "python-format",
            split: S::Train,
            task: T::StructuredIrGeneration,
            language: L::Python,
            source: SeedSource::Inline("^python\nx = 7\nprint(f\"value={x}\")\n"),
            label_source: None,
            expected_stdout: "value=7\n",
            provenance_path: "crates/linxts-compiler/src/parser.rs",
            provenance_reference: "parses_cross_language_format_forms",
        },
        SeedSpec {
            id: "javascript-template",
            split: S::Train,
            task: T::StructuredIrGeneration,
            language: L::Javascript,
            source: SeedSource::Inline("^javascript\nlet x = 7; console.log(`value=${x}`);\n"),
            label_source: None,
            expected_stdout: "value=7\n",
            provenance_path: "crates/linxts-runtime/tests/compiler_vm.rs",
            provenance_reference: "cross_language_formatting_shares_variables",
        },
        SeedSpec {
            id: "rust-format",
            split: S::Train,
            task: T::StructuredIrGeneration,
            language: L::Rust,
            source: SeedSource::Inline("^rust\nlet x = 7; println!(\"value={}\", x);\n"),
            label_source: None,
            expected_stdout: "value=7\n",
            provenance_path: "crates/linxts-runtime/tests/compiler_vm.rs",
            provenance_reference: "cross_language_formatting_shares_variables",
        },
        SeedSpec {
            id: "c-format",
            split: S::Train,
            task: T::StructuredIrGeneration,
            language: L::C,
            source: SeedSource::Inline("^c\nint x = 7; printf(\"value=%d\", x);\n"),
            label_source: None,
            expected_stdout: "value=7\n",
            provenance_path: "crates/linxts-compiler/src/parser.rs",
            provenance_reference: "parses_cross_language_format_forms",
        },
        SeedSpec {
            id: "invalid-generic-ir",
            split: S::Train,
            task: T::InvalidIrCorrection,
            language: L::Generic,
            source: SeedSource::Inline("x = 2\nprint(x + 3)\n"),
            label_source: None,
            expected_stdout: "5\n",
            provenance_path: "crates/linxts-ir/src/lib.rs",
            provenance_reference: "schema version rejection",
        },
        SeedSpec {
            id: "traditional-python-hint",
            split: S::Train,
            task: T::LanguageDetection,
            language: L::Python,
            source: SeedSource::Inline("^這是python\nprint(\"你好\")\n"),
            label_source: None,
            expected_stdout: "你好\n",
            provenance_path: "crates/linxts-compiler/src/hints.rs",
            provenance_reference: "Traditional Chinese language aliases",
        },
        SeedSpec {
            id: "python-range",
            split: S::Train,
            task: T::StructuredIrGeneration,
            language: L::Python,
            source: SeedSource::Inline(
                "^python\ntotal = 0\nfor i in range(1, 5):\n    total = total + i\nprint(total)\n",
            ),
            label_source: None,
            expected_stdout: "10\n",
            provenance_path: "crates/linxts-runtime/tests/compiler_vm.rs",
            provenance_reference: "python_indentation_and_range_execute",
        },
        SeedSpec {
            id: "c-detection",
            split: S::Validation,
            task: T::LanguageDetection,
            language: L::C,
            source: SeedSource::Inline("^C語言\nint x = 3; printf(\"%d\", x);\n"),
            label_source: None,
            expected_stdout: "3\n",
            provenance_path: "examples/c-basic.lx",
            provenance_reference: "C hint and printf subset",
        },
        SeedSpec {
            id: "invalid-format-ir",
            split: S::Validation,
            task: T::InvalidIrCorrection,
            language: L::Python,
            source: SeedSource::Inline("^python\nx = 9\nprint(f\"x={x}\")\n"),
            label_source: None,
            expected_stdout: "x=9\n",
            provenance_path: "crates/linxts-ir/src/lib.rs",
            provenance_reference: "format scope validation",
        },
        SeedSpec {
            id: "python-missing-quote",
            split: S::Validation,
            task: T::SyntaxRepair,
            language: L::Python,
            source: SeedSource::Inline("^python\nprint(\"修復成功\n"),
            label_source: None,
            expected_stdout: "修復成功\n",
            provenance_path: "crates/linxts-compiler/src/repair.rs",
            provenance_reference: "unterminated quote repair",
        },
        SeedSpec {
            id: "mixed-function-validation",
            split: S::Validation,
            task: T::MixedLanguageUnderstanding,
            language: L::Mixed,
            source: SeedSource::Inline(
                "^python\ndef add_one(value):\n    return value + 1\nseed = 4\n^javascript\nlet shared = add_one(seed);\nconsole.log(`js=${shared}`);\n^c\nprintf(\"c=%d\", shared);\n",
            ),
            label_source: None,
            expected_stdout: "js=5\nc=5\n",
            provenance_path: "crates/linxts-runtime/tests/compiler_vm.rs",
            provenance_reference: "mixed_language_functions_loops_and_formatting_execute",
        },
        SeedSpec {
            id: "javascript-holdout-repair",
            split: S::Test,
            task: T::SyntaxRepair,
            language: L::Javascript,
            source: SeedSource::Inline("^javascript\nconsole.log(\"保留測試\"\n"),
            label_source: None,
            expected_stdout: "保留測試\n",
            provenance_path: "training fixed holdout",
            provenance_reference: "unseen Traditional Chinese repair case",
        },
        SeedSpec {
            id: "natural-even-holdout",
            split: S::Test,
            task: T::NaturalLanguageToIr,
            language: L::Natural,
            source: SeedSource::Inline("^自然語言\n請顯示1到6之間的偶數\n"),
            label_source: None,
            expected_stdout: "2\n4\n6\n",
            provenance_path: "crates/linxts-compiler/src/lib.rs",
            provenance_reference: "deterministic even-range intent",
        },
        SeedSpec {
            id: "mixed-loop-holdout",
            split: S::Test,
            task: T::MixedLanguageUnderstanding,
            language: L::Mixed,
            source: SeedSource::Inline(
                "^python\nx = 2\n^rust\nwhile x < 5 { x = x + 1; }\nprintln!(\"x={}\", x);\n^c\nprintf(\"done=%d\", x);\n",
            ),
            label_source: None,
            expected_stdout: "x=5\ndone=5\n",
            provenance_path: "training fixed holdout",
            provenance_reference: "shared variable loop and formatting",
        },
        SeedSpec {
            id: "c-format-holdout",
            split: S::Test,
            task: T::StructuredIrGeneration,
            language: L::C,
            source: SeedSource::Inline("^c\nint answer = 40 + 2; printf(\"答案=%d\", answer);\n"),
            label_source: None,
            expected_stdout: "答案=42\n",
            provenance_path: "training fixed holdout",
            provenance_reference: "Traditional Chinese C format output",
        },
        SeedSpec {
            id: "natural-factorial-fallback",
            split: S::Test,
            task: T::NaturalLanguageToIr,
            language: L::Natural,
            source: SeedSource::Inline(
                "^自然語言\n請計算 1 到 6 的連乘積（也就是 6 的階乘），只輸出整數結果。\n",
            ),
            label_source: Some(
                "result = 1\ni = 1\nwhile i <= 6 { result = result * i; i = i + 1; }\nprint(result)\n",
            ),
            expected_stdout: "720\n",
            provenance_path: "tests/fixtures/ai/real-escalation.lx",
            provenance_reference: "fixed real-model fallback holdout with compiler-built oracle",
        },
    ]
}
