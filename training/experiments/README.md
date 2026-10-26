# Experiment records

Commit small, text-only experiment manifests and metric summaries here. Never commit model weights, adapters, optimizer states, generated datasets, raw caches, credentials, or source prompts that are not already public repository fixtures.

Each real run record must include:

- date and operator-provided run ID;
- Linxts Git commit and dirty state;
- dataset manifest and split hashes;
- base model ID, immutable revision, license, and local artifact hashes;
- Python and package lock hash;
- GPU, driver, ROCm, PyTorch HIP, and training framework versions;
- full configuration hash and random seeds;
- smoke-test result and peak VRAM before a full run;
- checkpoint metrics from the independent benchmark;
- selected checkpoint rationale;
- merged model and GGUF hashes when produced;
- known failures and whether the run is reproducible.

`template.json` is a schema-shaped starting point, not evidence that an experiment ran.
