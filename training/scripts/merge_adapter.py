#!/usr/bin/env python3
"""Merge a local PEFT adapter into a local base model without network access."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--adapter", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--revision")
    arguments = parser.parse_args()

    if not arguments.model.is_dir():
        parser.error("--model must be an existing local directory; downloads are disabled")
    if not arguments.adapter.is_dir():
        parser.error("--adapter must be an existing local directory")
    if arguments.output.exists():
        parser.error("refusing to overwrite existing merged model output")
    missing = [name for name in ("torch", "transformers", "peft") if version(name) is None]
    if missing:
        parser.error(f"missing merge dependencies: {', '.join(missing)}")

    import torch
    from peft import PeftModel
    from transformers import AutoModelForCausalLM, AutoTokenizer

    model_options: dict[str, Any] = {
        "local_files_only": True,
        "trust_remote_code": False,
        "dtype": torch.float16,
    }
    if arguments.revision:
        model_options["revision"] = arguments.revision
    base = AutoModelForCausalLM.from_pretrained(arguments.model, **model_options)
    adapted = PeftModel.from_pretrained(
        base,
        arguments.adapter,
        local_files_only=True,
        is_trainable=False,
    )
    merged = adapted.merge_and_unload(safe_merge=True)
    tokenizer = AutoTokenizer.from_pretrained(
        arguments.adapter,
        local_files_only=True,
        trust_remote_code=False,
    )

    temporary = arguments.output.with_name(
        f".{arguments.output.name}.tmp-{os.getpid()}"
    )
    if temporary.exists():
        parser.error(f"temporary output already exists: {temporary}")
    try:
        temporary.mkdir(parents=True)
        merged.save_pretrained(
            temporary,
            safe_serialization=True,
            max_shard_size="2GB",
        )
        tokenizer.save_pretrained(temporary)
        weight_files = sorted(temporary.glob("*.safetensors"))
        if not weight_files:
            raise RuntimeError("merged model did not produce safetensors weights")
        adapter_weights = arguments.adapter / "adapter_model.safetensors"
        manifest = {
            "status": "merged_local_only",
            "created_at_utc": datetime.now(timezone.utc).isoformat(),
            "base_model": str(arguments.model.resolve()),
            "base_revision": arguments.revision,
            "adapter": str(arguments.adapter.resolve()),
            "adapter_sha256": sha256(adapter_weights) if adapter_weights.is_file() else None,
            "output": str(arguments.output.resolve()),
            "weights": [
                {
                    "file": path.name,
                    "bytes": path.stat().st_size,
                    "sha256": sha256(path),
                }
                for path in weight_files
            ],
            "packages": {
                "torch": version("torch"),
                "transformers": version("transformers"),
                "peft": version("peft"),
            },
            "trust_remote_code": False,
            "local_files_only": True,
        }
        (temporary / "linxts-merge-manifest.json").write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        if arguments.output.exists():
            raise RuntimeError("merged model output appeared while merge was running")
        temporary.rename(arguments.output)
    except Exception:
        if temporary.exists():
            shutil.rmtree(temporary)
        raise

    print(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError) as error:
        print(f"linxts-mini merge failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
