#!/usr/bin/env python3
"""Inspect the local Linxts-Mini training environment without changing it."""

from __future__ import annotations

import argparse
import importlib.metadata
import importlib.util
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

PACKAGES = ("torch", "transformers", "peft", "datasets", "accelerate", "trl", "bitsandbytes")
TOOLS = ("rocminfo", "rocm-smi", "hipcc", "amd-smi", "vulkaninfo", "uv")


def package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def detect() -> dict[str, object]:
    result: dict[str, object] = {
        "python": {
            "version": platform.python_version(),
            "executable": sys.executable,
            "implementation": platform.python_implementation(),
        },
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
        },
        "tools": {name: shutil.which(name) for name in TOOLS},
        "packages": {name: package_version(name) for name in PACKAGES},
        "device_nodes": {
            "/dev/kfd": Path("/dev/kfd").exists(),
            "/dev/dri/renderD128": Path("/dev/dri/renderD128").exists(),
        },
    }
    if shutil.which("lspci"):
        command = subprocess.run(
            ["lspci", "-nnk"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
        result["display_controllers"] = [
            line.strip()
            for line in command.stdout.splitlines()
            if "VGA compatible controller" in line or "3D controller" in line
        ]

    torch_status: dict[str, object] = {"installed": False}
    if importlib.util.find_spec("torch") is not None:
        import torch

        torch_status = {
            "installed": True,
            "version": torch.__version__,
            "cuda_version": torch.version.cuda,
            "hip_version": torch.version.hip,
            "accelerator_available": torch.cuda.is_available(),
            "device_count": torch.cuda.device_count(),
            "devices": [],
        }
        if torch.cuda.is_available():
            devices = []
            for index in range(torch.cuda.device_count()):
                properties = torch.cuda.get_device_properties(index)
                devices.append(
                    {
                        "index": index,
                        "name": properties.name,
                        "total_memory": properties.total_memory,
                    }
                )
            torch_status["devices"] = devices
    result["torch"] = torch_status
    result["training_ready"] = bool(
        torch_status.get("installed")
        and torch_status.get("hip_version")
        and torch_status.get("accelerator_available")
        and result["packages"]["transformers"]
        and result["packages"]["peft"]
        and result["packages"]["accelerate"]
    )
    result["environment"] = {
        "HSA_OVERRIDE_GFX_VERSION_set": "HSA_OVERRIDE_GFX_VERSION" in os.environ,
        "HIP_VISIBLE_DEVICES_set": "HIP_VISIBLE_DEVICES" in os.environ,
    }
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-training-ready", action="store_true")
    arguments = parser.parse_args()
    report = detect()
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True)
    print(encoded)
    if arguments.output:
        if arguments.output.exists():
            parser.error(f"refusing to overwrite {arguments.output}")
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(encoded + "\n", encoding="utf-8")
    if arguments.require_training_ready and not report["training_ready"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
