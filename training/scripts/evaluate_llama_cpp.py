#!/usr/bin/env python3
"""Evaluate a loopback llama.cpp server against a frozen Linxts dataset split."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linxts_format import format_record  # noqa: E402


def loopback_base_url(value: str) -> str:
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme != "http" or parsed.hostname not in {"127.0.0.1", "localhost", "::1"}:
        raise argparse.ArgumentTypeError("llama.cpp URL must be loopback HTTP")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise argparse.ArgumentTypeError("llama.cpp URL must not contain credentials or query data")
    if parsed.path not in {"", "/"}:
        raise argparse.ArgumentTypeError("llama.cpp URL must not contain a path")
    return value.rstrip("/")


def request_json(url: str, method: str, payload: dict[str, Any] | None, timeout: float) -> dict[str, Any]:
    encoded = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode()
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=encoded, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        if response.status < 200 or response.status >= 300:
            raise RuntimeError(f"llama.cpp returned HTTP {response.status}")
        value = json.load(response)
    if not isinstance(value, dict):
        raise RuntimeError("llama.cpp response must be a JSON object")
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", type=loopback_base_url, default="http://127.0.0.1:8079")
    parser.add_argument("--dataset", required=True, type=Path)
    parser.add_argument("--split", choices=("train", "validation", "test"), default="test")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--model-id", required=True)
    parser.add_argument("--max-new-tokens", type=int, default=2048)
    parser.add_argument("--timeout-seconds", type=float, default=180.0)
    arguments = parser.parse_args()

    manifest_path = arguments.output.with_suffix(arguments.output.suffix + ".manifest.json")
    if arguments.output.exists() or manifest_path.exists():
        parser.error("refusing to overwrite prediction output or manifest")
    if arguments.max_new_tokens < 1 or arguments.timeout_seconds <= 0:
        parser.error("token and timeout limits must be positive")
    split_path = arguments.dataset / f"{arguments.split}.jsonl"
    dataset_manifest_path = arguments.dataset / "manifest.json"
    if not split_path.is_file() or not dataset_manifest_path.is_file():
        parser.error("dataset split and manifest must exist locally")

    health = request_json(f"{arguments.url}/health", "GET", None, arguments.timeout_seconds)
    records = [
        json.loads(line)
        for line in split_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    with arguments.output.open("x", encoding="utf-8") as handle:
        for record in records:
            prompt, _ = format_record(record)
            started = time.perf_counter()
            response = request_json(
                f"{arguments.url}/completion",
                "POST",
                {
                    "prompt": prompt,
                    "n_predict": arguments.max_new_tokens,
                    "temperature": 0.0,
                    "seed": 20261026,
                    "cache_prompt": False,
                    "stream": False,
                },
                arguments.timeout_seconds,
            )
            latency_ms = (time.perf_counter() - started) * 1000.0
            content = response.get("content")
            if not isinstance(content, str):
                raise RuntimeError("llama.cpp completion omitted string field `content`")
            try:
                candidate: Any = json.loads(content.strip())
            except json.JSONDecodeError:
                candidate = content.strip()
            prediction = {
                "sample_id": record["id"],
                "candidate": candidate,
                "latency_ms": latency_ms,
                "prompt_tokens": response.get("tokens_evaluated"),
                "completion_tokens": response.get("tokens_predicted"),
            }
            handle.write(json.dumps(prediction, ensure_ascii=False, separators=(",", ":")))
            handle.write("\n")
            handle.flush()

    manifest = {
        "server_url": arguments.url,
        "server_health": health,
        "model_id": arguments.model_id,
        "dataset_manifest": json.loads(dataset_manifest_path.read_text(encoding="utf-8")),
        "split": arguments.split,
        "split_sha256": hashlib.sha256(split_path.read_bytes()).hexdigest(),
        "records": len(records),
        "max_new_tokens": arguments.max_new_tokens,
        "temperature": 0.0,
        "seed": 20261026,
    }
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"status": "ok", "predictions": str(arguments.output)}, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, urllib.error.URLError, ValueError) as error:
        print(f"llama.cpp evaluation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
