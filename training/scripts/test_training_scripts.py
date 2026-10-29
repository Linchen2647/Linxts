from __future__ import annotations

import json
import ast
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "training" / "scripts"


class TrainingScriptTests(unittest.TestCase):
    def test_all_training_scripts_parse(self) -> None:
        for path in SCRIPTS.glob("*.py"):
            ast.parse(path.read_text(encoding="utf-8"), filename=str(path))

    def test_experiment_records_are_valid_json(self) -> None:
        experiments = ROOT / "training" / "experiments"
        for path in experiments.glob("*.json"):
            json.loads(path.read_text(encoding="utf-8"))

    def test_environment_probe_is_read_only_and_structured(self) -> None:
        result = subprocess.run(
            [sys.executable, "-I", str(SCRIPTS / "check_environment.py")],
            check=True,
            capture_output=True,
            text=True,
        )
        report = json.loads(result.stdout)
        self.assertIn("training_ready", report)
        self.assertIn("torch", report)
        self.assertIn("packages", report)

    def test_evaluator_refuses_missing_local_model(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                str(SCRIPTS / "evaluate_model.py"),
                "--model",
                "/definitely/missing/linxts-model",
                "--dataset",
                "/definitely/missing/dataset",
                "--output",
                "/definitely/missing/output.jsonl",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("downloads are disabled", result.stderr)

    def test_gguf_converter_requires_local_pinned_source(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                str(SCRIPTS / "convert_gguf.py"),
                "--llama-source",
                "/definitely/missing/llama.cpp",
                "--expected-commit",
                "deadbeef",
                "--model",
                "/definitely/missing/model",
                "--output",
                "/definitely/missing/model.gguf",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("local llama.cpp Git checkout", result.stderr)

    def test_llama_evaluator_rejects_non_loopback_url(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                str(SCRIPTS / "evaluate_llama_cpp.py"),
                "--url",
                "https://example.com",
                "--dataset",
                "/definitely/missing/dataset",
                "--output",
                "/definitely/missing/output.jsonl",
                "--model-id",
                "test-model",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("loopback HTTP", result.stderr)

    def test_merger_refuses_missing_local_inputs(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                str(SCRIPTS / "merge_adapter.py"),
                "--model",
                "/definitely/missing/linxts-model",
                "--adapter",
                "/definitely/missing/linxts-adapter",
                "--output",
                "/definitely/missing/merged-model",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("downloads are disabled", result.stderr)

    def test_format_smoke_excludes_verification_oracle(self) -> None:
        with tempfile.TemporaryDirectory(prefix="linxts-training-script-") as temporary:
            root = Path(temporary)
            dataset = root / "dataset"
            dataset.mkdir()
            record = {
                "id": "format-smoke",
                "task": "structured_ir_generation",
                "input": {"source": "PRINT 1\n", "diagnostics": []},
                "target": {"schema_version": "1", "status": "candidate"},
                "verification": {
                    "label_source": "must not enter prompt",
                    "source_hash": "test-source-hash",
                },
            }
            encoded = json.dumps(record, ensure_ascii=False) + "\n"
            (dataset / "train.jsonl").write_text(encoded, encoding="utf-8")
            (dataset / "validation.jsonl").write_text(encoded, encoding="utf-8")
            config = root / "config.toml"
            config.write_text(
                f'[dataset]\npath = "{dataset}"\nmax_length = 128\n',
                encoding="utf-8",
            )
            result = subprocess.run(
                [
                    sys.executable,
                    "-I",
                    str(SCRIPTS / "train_lora.py"),
                    "--config",
                    str(config),
                    "--format-smoke",
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            report = json.loads(result.stdout)
            self.assertFalse(report["label_source_exposed"])
            self.assertEqual(report["records"], 1)


if __name__ == "__main__":
    unittest.main()
