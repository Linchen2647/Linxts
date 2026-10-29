#!/usr/bin/env python3
"""Bounded ROCm/PyTorch forward-backward compatibility smoke test."""

from __future__ import annotations

import json
import sys
import time


def main() -> int:
    try:
        import torch
    except ImportError as error:
        print(f"torch import failed: {error}", file=sys.stderr)
        return 1
    if torch.version.hip is None:
        print("active PyTorch is not a ROCm/HIP build", file=sys.stderr)
        return 1
    if not torch.cuda.is_available():
        print("ROCm accelerator is unavailable", file=sys.stderr)
        return 1

    torch.manual_seed(20261026)
    torch.cuda.manual_seed_all(20261026)
    device = torch.device("cuda:0")
    memory_stats_available = True
    try:
        torch.cuda.reset_peak_memory_stats()
    except RuntimeError:
        memory_stats_available = False
    layer = torch.nn.Linear(1024, 1024, bias=True, device=device, dtype=torch.float32)
    optimizer = torch.optim.AdamW(layer.parameters(), lr=1e-4)
    values = torch.randn(8, 1024, device=device, dtype=torch.float32)
    target = torch.zeros(8, 1024, device=device, dtype=torch.float32)
    torch.cuda.synchronize()
    started = time.perf_counter()
    losses = []
    for _ in range(2):
        optimizer.zero_grad(set_to_none=True)
        output = layer(values)
        loss = torch.nn.functional.mse_loss(output, target)
        loss.backward()
        optimizer.step()
        losses.append(float(loss.detach().cpu()))
    torch.cuda.synchronize()
    report = {
        "status": "pass",
        "torch": torch.__version__,
        "torch_hip": torch.version.hip,
        "device": torch.cuda.get_device_name(0),
        "device_count": torch.cuda.device_count(),
        "iterations": 2,
        "losses": losses,
        "elapsed_ms": (time.perf_counter() - started) * 1000.0,
        "peak_memory_bytes": (
            torch.cuda.max_memory_allocated() if memory_stats_available else None
        ),
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
