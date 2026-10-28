# Local AI environment assessment

Assessment updated: 2026-10-28. This is an observed development-machine snapshot with measured Linxts-Mini Vulkan inference; it does not certify untested 3B or 7B artifacts.

## Observed state

| Component | Result |
| --- | --- |
| `llama-server` / `llama-cli` | Not in system `PATH`; approved release `b11910` Vulkan binaries are pinned under ignored `training/tools/` |
| Linxts local model endpoints | Mini was tested transiently on loopback port 8079; 3B, 7B, and larger-local tiers remain unconfigured |
| GPU | AMD Radeon RX 6800 XT (`amdgpu`, RADV NAVI21) |
| Vulkan | Available; instance 1.4.363, RADV device API 1.4.354 |
| Device-local Vulkan heap | 15.98 GiB, approximately 15.36 GiB available during assessment |
| ROCm | System `rocminfo` unavailable; approved user-local PyTorch ROCm 7.14 training works |
| OpenCL | `clinfo` is installed but reported no platforms |
| Build prerequisites | C compiler and `pkg-config` available; `cmake` and `ninja` unavailable |
| Host resources | 31 GiB RAM, 16 logical processors, no swap |

The initial commands were read-only environment checks (`command -v`, `lspci`, `vulkaninfo`, `clinfo`, `free`, `nproc`) plus `linxts doctor` and `linxts models list`. Later approved work installed only a user-local ROCm PyTorch environment, downloaded a revision-pinned 0.5B training checkpoint, and placed SHA-256-verified llama.cpp `b11910` source and Vulkan binaries under ignored workspace directories. No system package, driver, or system setting was changed.

## Assessment

The Vulkan path is now measured for Linxts-Mini. llama.cpp identified `Vulkan0: AMD Radeon RX 6800 XT (RADV NAVI21)`, loaded all Mini layers, and served both F16 and Q4_K_M GGUF artifacts. Q4_K_M averaged 1.51 seconds across the fixed 21-case split and passed all 17 AI-required executions. Capacity for separate 3B and 7B artifacts still must be measured with their exact approved files.

ROCm is not currently testable because its tooling is absent. Installing ROCm solely for this acceptance pass is unnecessary unless Vulkan fails or a later benchmark demonstrates a reason to compare backends. OpenCL is not a current fallback because no platform was reported.

A system llama.cpp build is still unavailable because CMake is absent, but the approved, hash-verified Vulkan release binary completed Mini acceptance without sudo or package installation. Any future source build, driver change, or additional model acquisition still requires explicit approval.

## Proposed local topology

Once the prerequisites and exact model artifacts are approved:

- launch the accepted Mini server on loopback port `8079`;
- launch the 3B server on loopback port `8080`;
- launch the 7B server on loopback port `8081`;
- optionally launch a larger local tier on loopback port `8082`;
- use llama.cpp's native `/health` and `/completion` routes;
- set `LINXTS_LLAMA_MINI_URL`, `LINXTS_LLAMA_MINI_MODEL`, and any approved 3B/7B variables only for the acceptance shell;
- keep online providers unconfigured and use `--offline` during local acceptance.

Before claiming hardware acceleration, record the llama.cpp version and build flags, confirm that server startup identifies the Vulkan backend and offloaded layers, and capture the health response. CPU fallback alone does not prove Vulkan inference.
