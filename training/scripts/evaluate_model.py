#!/usr/bin/env python3
"""Run local-only model inference and emit independent benchmark predictions."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linxts_format import format_record  # noqa: E402


def version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def load_records(dataset: Path, split: str) -> list[dict[str, Any]]:
    path = dataset / f"{split}.jsonl"
    with path.open(encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--adapter", type=Path)
    parser.add_argument("--dataset", required=True, type=Path)
    parser.add_argument("--split", choices=("train", "validation", "test"), default="test")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--max-new-tokens", type=int, default=2048)
    parser.add_argument("--allow-cpu", action="store_true")
    arguments = parser.parse_args()

    if not arguments.model.is_dir():
        parser.error("--model must be an existing local directory; downloads are disabled")
    if arguments.adapter is not None and not arguments.adapter.is_dir():
        parser.error("--adapter must be an existing local directory")
    manifest_path = arguments.output.with_suffix(arguments.output.suffix + ".manifest.json")
    if arguments.output.exists() or manifest_path.exists():
        parser.error("refusing to overwrite prediction output or manifest")
    if version("torch") is None or version("transformers") is None:
        parser.error("torch and transformers must be installed in the active environment")

    import torch
    from transformers import AutoModelForCausalLM, AutoTokenizer

    if not torch.cuda.is_available() and not arguments.allow_cpu:
        parser.error("accelerator unavailable; pass --allow-cpu only for a deliberate CPU run")
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    dtype = torch.float16 if torch.cuda.is_available() else torch.float32
    tokenizer = AutoTokenizer.from_pretrained(
        arguments.model,
        local_files_only=True,
        trust_remote_code=False,
    )
    if tokenizer.pad_token_id is None:
        tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(
        arguments.model,
        local_files_only=True,
        trust_remote_code=False,
        dtype=dtype,
    )
    if arguments.adapter is not None:
        if version("peft") is None:
            parser.error("PEFT is required when --adapter is used")
        from peft import PeftModel

        model = PeftModel.from_pretrained(
            model,
            arguments.adapter,
            local_files_only=True,
            is_trainable=False,
        )
    model.to(device)
    model.eval()

    records = load_records(arguments.dataset, arguments.split)
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    with arguments.output.open("x", encoding="utf-8") as handle:
        for record in records:
            prompt, _ = format_record(record)
            encoded = tokenizer(prompt, return_tensors="pt")
            encoded = {key: value.to(device) for key, value in encoded.items()}
            if device.type == "cuda":
                torch.cuda.synchronize()
            started = time.perf_counter()
            with torch.inference_mode():
                generated = model.generate(
                    **encoded,
                    do_sample=False,
                    max_new_tokens=arguments.max_new_tokens,
                    pad_token_id=tokenizer.pad_token_id,
                    eos_token_id=tokenizer.eos_token_id,
                )
            if device.type == "cuda":
                torch.cuda.synchronize()
            latency_ms = (time.perf_counter() - started) * 1000.0
            prompt_tokens = int(encoded["input_ids"].shape[-1])
            completion_ids = generated[0, prompt_tokens:]
            content = tokenizer.decode(completion_ids, skip_special_tokens=True).strip()
            try:
                candidate: Any = json.loads(content)
            except json.JSONDecodeError:
                candidate = content
            prediction = {
                "sample_id": record["id"],
                "candidate": candidate,
                "latency_ms": latency_ms,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": int(completion_ids.shape[-1]),
            }
            handle.write(json.dumps(prediction, ensure_ascii=False, separators=(",", ":")))
            handle.write("\n")
            handle.flush()

    split_bytes = (arguments.dataset / f"{arguments.split}.jsonl").read_bytes()
    manifest = {
        "model": str(arguments.model.resolve()),
        "adapter": str(arguments.adapter.resolve()) if arguments.adapter else None,
        "dataset_manifest": json.loads(
            (arguments.dataset / "manifest.json").read_text(encoding="utf-8")
        ),
        "split": arguments.split,
        "split_sha256": hashlib.sha256(split_bytes).hexdigest(),
        "records": len(records),
        "max_new_tokens": arguments.max_new_tokens,
        "decoding": "greedy",
        "device": str(device),
        "device_name": torch.cuda.get_device_name(0) if torch.cuda.is_available() else "cpu",
        "torch": version("torch"),
        "transformers": version("transformers"),
        "peft": version("peft"),
    }
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"status": "ok", "predictions": str(arguments.output)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
