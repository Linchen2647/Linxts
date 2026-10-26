"""Shared, dependency-free Linxts-Mini prompt formatting."""

from __future__ import annotations

import json
from typing import Any

SYSTEM_PROMPT = (
    "You are Linxts-Mini, the first Linxts compiler fallback. Return exactly one JSON "
    "CandidateEnvelope matching the current schema. Never emit markdown or unvalidated code."
)


def format_record(record: dict[str, Any]) -> tuple[str, str]:
    model_input = {
        "task": record["task"],
        "source": record["input"]["source"],
        "source_hash": record["verification"]["source_hash"],
        "diagnostics": record["input"]["diagnostics"],
    }
    if "invalid_ir" in record["input"]:
        model_input["invalid_ir"] = record["input"]["invalid_ir"]
    prompt = (
        f"{SYSTEM_PROMPT}\n"
        f"INPUT JSON:\n{json.dumps(model_input, ensure_ascii=False, separators=(',', ':'))}\n"
        "OUTPUT JSON:\n"
    )
    target = json.dumps(record["target"], ensure_ascii=False, separators=(",", ":"))
    return prompt, target
