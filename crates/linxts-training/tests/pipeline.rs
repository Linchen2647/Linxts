use linxts_training::{
    BenchmarkReport, DEFAULT_DATASET_SEED, DatasetSplit, PredictionRecord, TrainingSample,
    benchmark_no_ai, benchmark_predictions, generate_dataset, seed_count, validate_dataset,
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn temporary_directory(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "linxts-training-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn generate(label: &str) -> PathBuf {
    let directory = temporary_directory(label);
    generate_dataset(&repository_root(), &directory, DEFAULT_DATASET_SEED)
        .expect("generate dataset");
    directory
}

fn samples(directory: &Path) -> Vec<TrainingSample> {
    let mut samples = Vec::new();
    for file in ["train.jsonl", "validation.jsonl", "test.jsonl"] {
        let text = std::fs::read_to_string(directory.join(file)).expect("read split");
        samples.extend(
            text.lines()
                .map(|line| serde_json::from_str(line).expect("decode sample")),
        );
    }
    samples
}

#[test]
fn generated_dataset_is_reproducible_and_valid() {
    let first = generate("repro-a");
    let second = generate("repro-b");

    let report = validate_dataset(&first).expect("validate dataset");
    assert!(report.passed(), "{report:?}");
    assert_eq!(seed_count(), 1_941);
    assert_eq!(report.total_samples, seed_count());
    for file in [
        "train.jsonl",
        "validation.jsonl",
        "test.jsonl",
        "manifest.json",
    ] {
        assert_eq!(
            std::fs::read(first.join(file)).expect("read first output"),
            std::fs::read(second.join(file)).expect("read second output"),
            "{file} must be reproducible"
        );
    }

    let generated = samples(&first);
    assert_eq!(
        generated
            .iter()
            .filter(|sample| sample.split == DatasetSplit::Test)
            .count(),
        21
    );
    assert_eq!(
        generated
            .iter()
            .filter(|sample| sample.group_id.starts_with("fallback-")
                && sample.split == DatasetSplit::Test)
            .count(),
        16
    );
    assert!(
        !generated
            .iter()
            .any(|sample| sample.group_id == "fallback-factorial-6"),
        "the fixed factorial-6 holdout must not have an equivalent generated group"
    );
    assert!(
        generated
            .iter()
            .any(|sample| sample.split == DatasetSplit::Test
                && !sample.verification.input_compiles_without_ai)
    );
    let _ = std::fs::remove_dir_all(first);
    let _ = std::fs::remove_dir_all(second);
}

#[test]
fn validator_rejects_tampered_labels() {
    let directory = generate("tamper");
    let path = directory.join("test.jsonl");
    let text = std::fs::read_to_string(&path).expect("read test split");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut sample: serde_json::Value =
        serde_json::from_str(&lines[0]).expect("decode sample JSON");
    sample["target"]["ir"]["schema_version"] = serde_json::json!("tampered");
    lines[0] = serde_json::to_string(&sample).expect("encode tampered sample");
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write tampered split");

    let report = validate_dataset(&directory).expect("validation report");
    assert!(!report.passed());
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("schema") || error.contains("fingerprint")),
        "{:?}",
        report.errors
    );
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn benchmark_compares_no_ai_and_prediction_outputs() {
    let directory = generate("benchmark");
    let baseline = benchmark_no_ai(&directory).expect("no-AI benchmark");
    assert_eq!(baseline.system, "linxts-no-ai");
    assert_eq!(baseline.total_samples, seed_count());
    assert!(baseline.execution_correctness < 1.0);
    assert!(baseline.escalation_required_rate > 0.0);

    let predictions_path = directory.join("perfect-predictions.jsonl");
    let mut predictions = String::new();
    for sample in samples(&directory) {
        let record = PredictionRecord {
            sample_id: sample.id,
            candidate: serde_json::to_value(sample.target).expect("candidate JSON"),
            latency_ms: Some(10.0),
            prompt_tokens: Some(100),
            completion_tokens: Some(200),
        };
        predictions.push_str(&serde_json::to_string(&record).expect("prediction JSON"));
        predictions.push('\n');
    }
    std::fs::write(&predictions_path, predictions).expect("write predictions");
    let model: BenchmarkReport =
        benchmark_predictions(&directory, &predictions_path, "oracle-model")
            .expect("prediction benchmark");
    assert_eq!(model.language_detection_accuracy, 1.0);
    assert_eq!(model.repair_parse_success_rate, 1.0);
    assert_eq!(model.ir_schema_validity, 1.0);
    assert_eq!(model.ir_validator_pass_rate, 1.0);
    assert_eq!(model.execution_correctness, 1.0);
    assert_eq!(model.escalation_required_rate, 0.0);
    assert_eq!(model.average_latency_ms, Some(10.0));
    assert_eq!(model.prompt_tokens, Some(seed_count() as u64 * 100));
    assert_eq!(model.completion_tokens, Some(seed_count() as u64 * 200));
    let _ = std::fs::remove_dir_all(directory);
}
