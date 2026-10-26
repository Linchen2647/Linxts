#!/usr/bin/env python3
"""Local-only LoRA/QLoRA training entrypoint for Linxts-Mini."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import inspect
import json
import random
import sys
import tomllib
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linxts_format import format_record  # noqa: E402


def load_config(path: Path) -> tuple[dict[str, Any], bytes]:
    raw = path.read_bytes()
    return tomllib.loads(raw.decode("utf-8")), raw


def load_records(dataset: Path, split: str, limit: int | None = None) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    with (dataset / f"{split}.jsonl").open(encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                records.append(json.loads(line))
                if limit is not None and len(records) >= limit:
                    break
    return records



def package_versions() -> dict[str, str | None]:
    versions: dict[str, str | None] = {}
    for name in ("torch", "transformers", "peft", "accelerate", "bitsandbytes"):
        try:
            versions[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            versions[name] = None
    return versions


def require_dependencies(quantization_mode: str) -> None:
    missing = [
        name
        for name in ("torch", "transformers", "peft", "accelerate")
        if package_versions()[name] is None
    ]
    if quantization_mode == "bnb_4bit" and package_versions()["bitsandbytes"] is None:
        missing.append("bitsandbytes")
    if missing:
        raise RuntimeError(f"missing training dependencies: {', '.join(sorted(set(missing)))}")


def find_checkpoint(output: Path) -> str | None:
    checkpoints = sorted(
        output.glob("checkpoint-*"),
        key=lambda path: int(path.name.rsplit("-", 1)[-1]),
    )
    return str(checkpoints[-1]) if checkpoints else None


@dataclass
class EncodedExample:
    input_ids: list[int]
    labels: list[int]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--smoke-test", action="store_true")
    parser.add_argument(
        "--format-smoke",
        action="store_true",
        help="validate prompt formatting without importing ML frameworks",
    )
    parser.add_argument("--resume", action="store_true")
    arguments = parser.parse_args()

    config, config_bytes = load_config(arguments.config)
    dataset = Path(config["dataset"]["path"])
    train_records = load_records(dataset, "train", 8 if arguments.smoke_test else None)
    validation_records = load_records(dataset, "validation", 4 if arguments.smoke_test else None)
    if not train_records or not validation_records:
        raise RuntimeError("training and validation splits must both be non-empty")
    formatted = [format_record(record) for record in train_records]
    if arguments.format_smoke:
        summary = {
            "records": len(formatted),
            "first_sample_id": train_records[0]["id"],
            "prompt_sha256": hashlib.sha256(formatted[0][0].encode()).hexdigest(),
            "target_sha256": hashlib.sha256(formatted[0][1].encode()).hexdigest(),
            "label_source_exposed": "label_source" in formatted[0][0],
        }
        print(json.dumps(summary, indent=2, sort_keys=True))
        return 0

    model_path = Path(config["model"]["path"])
    if not model_path.is_dir():
        raise RuntimeError(
            f"base model must already exist as a local directory; downloads are disabled: {model_path}"
        )
    quantization_mode = config["quantization"]["mode"]
    require_dependencies(quantization_mode)

    import torch
    from peft import LoraConfig, get_peft_model, prepare_model_for_kbit_training
    from torch.utils.data import Dataset
    from transformers import (
        AutoModelForCausalLM,
        AutoTokenizer,
        BitsAndBytesConfig,
        Trainer,
        TrainingArguments,
    )

    seed = int(config["training"]["seed"])
    random.seed(seed)
    torch.manual_seed(seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(seed)
    if not torch.cuda.is_available() and not config["training"].get("allow_cpu", False):
        raise RuntimeError("accelerator unavailable and allow_cpu is false")
    if torch.cuda.is_available() and torch.version.hip is None:
        raise RuntimeError("accelerator is not a ROCm/HIP PyTorch build")

    tokenizer = AutoTokenizer.from_pretrained(
        model_path,
        local_files_only=True,
        trust_remote_code=False,
        revision=config["model"].get("revision"),
    )
    if tokenizer.pad_token_id is None:
        tokenizer.pad_token = tokenizer.eos_token
    max_length = int(config["dataset"]["max_length"])

    class LinxtsDataset(Dataset):
        def __init__(self, records: list[dict[str, Any]]) -> None:
            self.examples = [self.encode(record) for record in records]

        def encode(self, record: dict[str, Any]) -> EncodedExample:
            prompt, target = format_record(record)
            prompt_ids = tokenizer(prompt, add_special_tokens=True)["input_ids"]
            target_ids = tokenizer(target, add_special_tokens=False)["input_ids"]
            if tokenizer.eos_token_id is not None:
                target_ids.append(tokenizer.eos_token_id)
            input_ids = (prompt_ids + target_ids)[:max_length]
            prompt_length = min(len(prompt_ids), len(input_ids))
            labels = [-100] * prompt_length + input_ids[prompt_length:]
            if all(label == -100 for label in labels):
                raise RuntimeError(f"sample {record['id']} has no target tokens after truncation")
            return EncodedExample(input_ids=input_ids, labels=labels)

        def __len__(self) -> int:
            return len(self.examples)

        def __getitem__(self, index: int) -> EncodedExample:
            return self.examples[index]

    class CausalCollator:
        def __call__(self, examples: list[EncodedExample]) -> dict[str, Any]:
            length = max(len(example.input_ids) for example in examples)
            input_ids = torch.full(
                (len(examples), length), tokenizer.pad_token_id, dtype=torch.long
            )
            labels = torch.full((len(examples), length), -100, dtype=torch.long)
            attention_mask = torch.zeros((len(examples), length), dtype=torch.long)
            for row, example in enumerate(examples):
                size = len(example.input_ids)
                input_ids[row, :size] = torch.tensor(example.input_ids)
                labels[row, :size] = torch.tensor(example.labels)
                attention_mask[row, :size] = 1
            return {
                "input_ids": input_ids,
                "labels": labels,
                "attention_mask": attention_mask,
            }

    bf16 = bool(config["training"]["bf16"])
    fp16 = bool(config["training"]["fp16"])
    if bf16 and torch.cuda.is_available() and not torch.cuda.is_bf16_supported():
        raise RuntimeError("bf16 was requested but the active accelerator does not report support")
    model_options: dict[str, Any] = {
        "local_files_only": True,
        "trust_remote_code": False,
        "revision": config["model"].get("revision"),
        "torch_dtype": torch.bfloat16 if bf16 else torch.float16 if fp16 else torch.float32,
    }
    if quantization_mode == "bnb_4bit":
        model_options["quantization_config"] = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_use_double_quant=True,
            bnb_4bit_compute_dtype=torch.bfloat16 if bf16 else torch.float16,
        )
    elif quantization_mode != "none":
        raise RuntimeError(f"unsupported quantization mode: {quantization_mode}")

    model = AutoModelForCausalLM.from_pretrained(model_path, **model_options)
    if quantization_mode == "bnb_4bit":
        model = prepare_model_for_kbit_training(model)
    if config["training"]["gradient_checkpointing"]:
        model.gradient_checkpointing_enable()
        model.config.use_cache = False
    model = get_peft_model(
        model,
        LoraConfig(
            task_type="CAUSAL_LM",
            inference_mode=False,
            r=int(config["lora"]["rank"]),
            lora_alpha=int(config["lora"]["alpha"]),
            lora_dropout=float(config["lora"]["dropout"]),
            target_modules=list(config["lora"]["target_modules"]),
        ),
    )

    if arguments.smoke_test:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        output = Path(config["output"]["root"]) / f"smoke-{stamp}"
        max_steps = 2
    else:
        output = Path(config["output"]["root"]) / config["output"]["run_name"]
        max_steps = int(config["training"].get("max_steps", -1))
    if arguments.smoke_test and arguments.resume:
        raise RuntimeError("smoke tests always start in a new output directory")
    if output.exists() and not arguments.resume:
        raise RuntimeError(f"refusing to overwrite existing run: {output}")
    checkpoint = find_checkpoint(output) if arguments.resume else None
    if arguments.resume and checkpoint is None:
        raise RuntimeError(f"resume requested but no checkpoint exists in {output}")
    output.mkdir(parents=True, exist_ok=True)

    config_sha256 = hashlib.sha256(config_bytes).hexdigest()
    dataset_manifest = json.loads((dataset / "manifest.json").read_text(encoding="utf-8"))
    manifest_path = output / "run-manifest.json"
    if arguments.resume and manifest_path.exists():
        previous = json.loads(manifest_path.read_text(encoding="utf-8"))
        if previous.get("config_sha256") != config_sha256:
            raise RuntimeError("resume configuration hash differs from the original run")
        if previous.get("dataset_manifest") != dataset_manifest:
            raise RuntimeError("resume dataset manifest differs from the original run")
    manifest = {
        "status": "running",
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "config": str(arguments.config),
        "config_sha256": config_sha256,
        "dataset_manifest": dataset_manifest,
        "base_model_path": str(model_path.resolve()),
        "base_model_revision": config["model"].get("revision"),
        "packages": package_versions(),
        "python": sys.version,
        "torch_hip": torch.version.hip,
        "device": torch.cuda.get_device_name(0) if torch.cuda.is_available() else "cpu",
        "seed": seed,
        "quantization": quantization_mode,
        "smoke_test": arguments.smoke_test,
        "max_steps": max_steps,
        "resumed_from": checkpoint,
    }
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    training_options: dict[str, Any] = {
        "output_dir": str(output),
        "seed": seed,
        "data_seed": seed,
        "num_train_epochs": float(config["training"]["epochs"]),
        "max_steps": max_steps,
        "per_device_train_batch_size": int(config["training"]["batch_size"]),
        "per_device_eval_batch_size": int(config["training"]["batch_size"]),
        "gradient_accumulation_steps": int(config["training"]["gradient_accumulation_steps"]),
        "learning_rate": float(config["training"]["learning_rate"]),
        "warmup_ratio": float(config["training"]["warmup_ratio"]),
        "weight_decay": float(config["training"]["weight_decay"]),
        "logging_steps": 1 if arguments.smoke_test else int(config["training"]["logging_steps"]),
        "save_steps": 1 if arguments.smoke_test else int(config["training"]["save_steps"]),
        "eval_steps": 1 if arguments.smoke_test else int(config["training"]["eval_steps"]),
        "save_strategy": "steps",
        "logging_strategy": "steps",
        "bf16": bf16,
        "fp16": fp16,
        "report_to": [],
        "remove_unused_columns": False,
        "save_total_limit": int(config["training"]["save_total_limit"]),
    }
    parameter_names = inspect.signature(TrainingArguments.__init__).parameters
    strategy_name = "eval_strategy" if "eval_strategy" in parameter_names else "evaluation_strategy"
    training_options[strategy_name] = "steps"
    training_arguments = TrainingArguments(**training_options)

    trainer_options: dict[str, Any] = {
        "model": model,
        "args": training_arguments,
        "train_dataset": LinxtsDataset(train_records),
        "eval_dataset": LinxtsDataset(validation_records),
        "data_collator": CausalCollator(),
    }
    if "processing_class" in inspect.signature(Trainer.__init__).parameters:
        trainer_options["processing_class"] = tokenizer
    else:
        trainer_options["tokenizer"] = tokenizer
    trainer = Trainer(**trainer_options)
    trainer.train(resume_from_checkpoint=checkpoint)
    trainer.save_model(str(output / "adapter"))
    tokenizer.save_pretrained(output / "adapter")

    manifest["status"] = "completed"
    manifest["completed_at_utc"] = datetime.now(timezone.utc).isoformat()
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"status": "ok", "output": str(output)}, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (KeyError, OSError, RuntimeError, ValueError) as error:
        print(f"linxts-mini training failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
