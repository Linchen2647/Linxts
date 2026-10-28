#!/usr/bin/env python3
"""Run a pinned local llama.cpp GGUF converter through an isolated wrapper."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

RUNNER = """import runpy,sys
source = sys.argv.pop(1)
converter = source + '/convert_hf_to_gguf.py'
sys.path.insert(0, source)
sys.argv[0] = converter
runpy.run_path(converter, run_name='__main__')
"""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--llama-source", required=True, type=Path)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--outtype",
        choices=("f32", "f16", "bf16", "q8_0", "tq1_0", "tq2_0", "auto"),
        default="f16",
    )
    parser.add_argument("--model-name", default="Linxts-Mini")
    arguments = parser.parse_args()

    converter = arguments.llama_source / "convert_hf_to_gguf.py"
    if not converter.is_file() or not (arguments.llama_source / ".git").is_dir():
        parser.error("--llama-source must be a local llama.cpp Git checkout")
    if not arguments.model.is_dir():
        parser.error("--model must be an existing local merged-model directory")
    if arguments.output.exists():
        parser.error("refusing to overwrite existing GGUF output")
    commit = subprocess.run(
        ["git", "-C", str(arguments.llama_source), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if commit != arguments.expected_commit:
        parser.error(f"llama.cpp commit mismatch: expected {arguments.expected_commit}, got {commit}")

    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = arguments.output.with_name(f".{arguments.output.name}.tmp-{os.getpid()}.gguf")
    command = [
        sys.executable,
        "-I",
        "-c",
        RUNNER,
        str(arguments.llama_source.resolve()),
        str(arguments.model.resolve()),
        "--outfile",
        str(temporary.resolve()),
        "--outtype",
        arguments.outtype,
        "--model-name",
        arguments.model_name,
    ]
    try:
        subprocess.run(command, check=True)
        if not temporary.is_file() or temporary.stat().st_size == 0:
            raise RuntimeError("converter did not produce a non-empty GGUF file")
        if arguments.output.exists():
            raise RuntimeError("GGUF output appeared while conversion was running")
        temporary.rename(arguments.output)
    except Exception:
        if temporary.exists():
            temporary.unlink()
        raise

    report = {
        "status": "converted_local_only",
        "llama_cpp_commit": commit,
        "model": str(arguments.model.resolve()),
        "output": str(arguments.output.resolve()),
        "outtype": arguments.outtype,
        "bytes": arguments.output.stat().st_size,
        "sha256": sha256(arguments.output),
    }
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Linxts GGUF conversion failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
