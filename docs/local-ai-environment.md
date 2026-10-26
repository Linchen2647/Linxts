# Local AI environment assessment

Assessment date: 2026-10-26. This is an observed development-machine snapshot, not proof that model inference works.

## Observed state

| Component | Result |
| --- | --- |
| `llama-server` / `llama-cli` | Not present in `PATH` |
| Linxts local model endpoints | 3B, 7B, and larger-local tiers are not configured |
| GPU | AMD Radeon RX 6800 XT (`amdgpu`, RADV NAVI21) |
| Vulkan | Available; instance 1.4.363, RADV device API 1.4.354 |
| Device-local Vulkan heap | 15.98 GiB, approximately 15.36 GiB available during assessment |
| ROCm | `rocminfo` unavailable |
| OpenCL | `clinfo` is installed but reported no platforms |
| Build prerequisites | C compiler and `pkg-config` available; `cmake` and `ninja` unavailable |
| Host resources | 31 GiB RAM, 16 logical processors, no swap |

Commands used were read-only environment checks (`command -v`, `lspci`, `vulkaninfo`, `clinfo`, `free`, `nproc`) plus `linxts doctor` and `linxts models list`. No package, driver, system setting, server, or model was installed or changed. No model was downloaded and no network inference was attempted.

## Assessment

The Vulkan path is the best current llama.cpp backend candidate. The GPU and RADV Vulkan device are visible, and the device-local heap should normally accommodate quantized 3B and 7B GGUF models, subject to the selected quantization, context length, KV-cache type, and GPU-layer configuration. Capacity must be measured with the exact model files; this document does not certify that any particular model fits or runs correctly.

ROCm is not currently testable because its tooling is absent. Installing ROCm solely for this acceptance pass is unnecessary unless Vulkan fails or a later benchmark demonstrates a reason to compare backends. OpenCL is not a current fallback because no platform was reported.

A local llama.cpp build is not presently ready because `llama-server` and the required CMake frontend are absent. Installing packages, compiling llama.cpp, or changing drivers requires a separate user-approved action. Model acquisition also requires explicit approval before any download.

## Proposed local topology

Once the prerequisites and exact model artifacts are approved:

- launch the 3B server on loopback port `8080`;
- launch the 7B server on loopback port `8081`;
- optionally launch a larger local tier on loopback port `8082`;
- use llama.cpp's native `/health` and `/completion` routes;
- set `LINXTS_LLAMA_3B_URL`, `LINXTS_LLAMA_3B_MODEL`, `LINXTS_LLAMA_7B_URL`, and `LINXTS_LLAMA_7B_MODEL` only for the acceptance shell;
- keep online providers unconfigured and use `--offline` during local acceptance.

Before claiming hardware acceleration, record the llama.cpp version and build flags, confirm that server startup identifies the Vulkan backend and offloaded layers, and capture the health response. CPU fallback alone does not prove Vulkan inference.
