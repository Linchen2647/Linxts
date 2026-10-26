# Linxts-Mini model and training assessment

Assessment date: 2026-10-26. No package or model was downloaded, no system setting was changed, and no GPU training was started during this assessment.

## Local environment

Observed directly on the development host:

- AMD Radeon RX 6800 XT (`amdgpu`, Navi 21) and `/dev/kfd` are present;
- Vulkan works, but `rocminfo`, `rocm-smi`, `hipcc`, and `amd-smi` are unavailable;
- Python is 3.14.8;
- PyTorch, Transformers, PEFT, Accelerate, TRL, Datasets, and bitsandbytes are not installed;
- `uv` and Python `venv` support are available;
- therefore the host is **not training-ready** and no real optimizer smoke test can honestly be reported as passed.

Upstream PyTorch lists Arch Linux under its general Linux support criteria and currently accepts Python 3.10–3.14. AMD's current Radeon ROCm matrix, however, does not list the RX 6800 XT among officially supported Radeon GPUs. The presence of `/dev/kfd` is not proof that this card and framework combination is supported. Any ROCm path on this machine must be treated as an unsupported compatibility experiment until a tensor allocation, forward/backward pass, and memory test succeed.

## Candidate models

| Candidate | Size | License | Code / Chinese / JSON suitability | GGUF path | Decision |
| --- | ---: | --- | --- | --- | --- |
| Qwen2.5-Coder-0.5B-Instruct | 0.49B | Apache-2.0 | Code-specialized, multilingual family, instruction output; strongest low-memory starting point | Official Qwen GGUF repository exists | **Primary smoke and first LoRA target** |
| Qwen2.5-Coder-1.5B-Instruct | 1.54B | Apache-2.0 | More capacity for nested IR and Chinese intent; slightly above the literal 1.5B boundary but marketed as 1.5B | Official Qwen GGUF repository exists | Quality candidate after 0.5B pipeline passes |
| DeepSeek-Coder-1.3B-Instruct | 1.3B | DeepSeek Model License for weights; code is MIT | Strong code focus and useful comparison, but license obligations are less simple than Apache-2.0 | llama.cpp conversion requires a pinned compatibility test | Secondary benchmark candidate |
| TinyLlama-1.1B-Chat-v1.0 | 1.1B | Apache-2.0 | English/general chat rather than code- and Chinese-focused | Common llama.cpp architecture | Control model, not preferred |

Qwen's official GGUF model cards list approximately 415–676 MB for 0.5B quantizations and 753 MB–1.89 GB for 1.5B quantizations. Training must use the original Transformers checkpoint rather than GGUF; GGUF is produced only after adapter evaluation and merge.

## LoRA versus QLoRA

Start with ordinary LoRA on the 0.5B model:

- fewer backend-specific components;
- no dependency on 4-bit CUDA kernels;
- easier to distinguish ROCm/PyTorch failure from quantization failure;
- expected to fit 16 GiB with batch size 1, sequence length 1024, gradient accumulation, and gradient checkpointing if the ROCm stack works.

QLoRA remains an experiment. Current bitsandbytes documentation advertises AMD support and includes `gfx1030` in Linux ROCm wheel targets, but the RX 6800 XT is absent from AMD's officially supported Radeon ROCm matrix. These statements do not together prove that NF4 training works on this exact host. The checked-in QLoRA configuration is disabled by policy until a dedicated import, 4-bit load, forward, backward, optimizer-step, and peak-memory smoke test passes.

Do not use CUDA-only Flash Attention, CUDA-specific fused optimizers, or an NVIDIA-only quantization assumption. The initial script uses standard Transformers Trainer, PEFT, PyTorch operations, and an explicit local-only model path.

## Planning estimates

These are conservative planning ranges, not measured results:

| Target | LoRA VRAM planning range | Experimental 4-bit range | Working disk budget | Initial 10k-example estimate if ROCm works |
| --- | ---: | ---: | ---: | ---: |
| Qwen2.5-Coder 0.5B | 4–8 GiB | 3–6 GiB | 12–20 GiB | roughly 2–8 hours for 1–3 epochs |
| Qwen2.5-Coder 1.5B | 9–15 GiB | 5–10 GiB | 25–40 GiB | roughly 6–24 hours for 1–3 epochs |

Actual memory depends on sequence length, tokenizer expansion of IR JSON, precision support, activation checkpointing, optimizer, and framework kernels. Actual time on an unsupported RX 6800 XT may be much slower or may fail entirely. The formal plan must replace these ranges with measurements from the two-step smoke run before scheduling long training. CPU fallback is suitable for data and formatting tests, not for estimating full fine-tuning duration.

## Reproducible pipeline

1. Generate and validate the versioned JSONL corpus with `linxts-training`.
2. Freeze split SHA-256 values and keep the test split untouched.
3. Run the no-AI Linxts benchmark.
4. Run base-model inference and store prediction JSONL without training.
5. Run `train_lora.py --format-smoke` to verify exact prompts and prevent oracle leakage.
6. After dependency and model approval, run `train_lora.py --smoke-test`; it performs two real optimizer steps and writes a run manifest.
7. Resume interrupted runs only from recorded `checkpoint-*` directories with `--resume`.
8. Evaluate every checkpoint with the independent Rust benchmark; training loss alone cannot select a model.
9. Select by execution correctness first, then validator pass, schema validity, escalation rate, latency, and tokens.
10. Merge only the selected adapter, convert the pinned merged checkpoint to GGUF, and verify llama.cpp output against the same test split.
11. Add Linxts-Mini as the first router tier only after end-to-end acceptance.

The training script calls `from_pretrained(..., local_files_only=True, trust_remote_code=False)`. It cannot silently download a model and refuses a missing local model directory. The smoke configuration uses a fixed seed, two steps, at most eight training and four validation records, cache-disabled data, and a new timestamped output directory.

## Approval gate

The next executable training step requires explicit approval for both:

1. creating an isolated Python environment and downloading/installing a pinned ROCm PyTorch + Transformers + PEFT toolchain; and
2. downloading one exact, revision-pinned base model—recommended first choice: Qwen2.5-Coder-0.5B-Instruct.

Until then, only dependency-free environment checks, deterministic data generation, prompt formatting, validation, and benchmarking may run. A formatting smoke test is not reported as a GPU training smoke test.

## Sources

- [Qwen2.5-Coder-0.5B-Instruct-GGUF official model card](https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF)
- [Qwen2.5-Coder-1.5B-Instruct-GGUF official repository](https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/tree/main)
- [DeepSeek-Coder-1.3B-Instruct model card and weight license](https://huggingface.co/deepseek-ai/deepseek-coder-1.3b-instruct)
- [TinyLlama-1.1B-Chat-v1.0 model card](https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0)
- [AMD Radeon ROCm Linux compatibility matrix](https://rocm.docs.amd.com/projects/radeon-ryzen/en/latest/docs/compatibility/compatibilityrad/native_linux/native_linux_compatibility.html)
- [ROCm Linux system requirements](https://github.com/ROCm/rocm-install-on-linux/blob/develop/docs/reference/system-requirements.rst)
- [PyTorch local installation and Linux requirements](https://pytorch.org/get-started/locally/)
- [bitsandbytes current installation and AMD targets](https://github.com/bitsandbytes-foundation/bitsandbytes/blob/main/docs/source/installation.mdx)
