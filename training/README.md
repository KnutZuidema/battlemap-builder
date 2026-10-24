# dungeonforge-training

`dungeonforge-training` is the Python package for Dungeon Forge LoRA fine-tuning configuration and run policy management.

Conforms to:
- [ADR-0008: FLUX 3 Image Klein LoRA training stack](../docs/adr/0008-flux-3-image-klein-lora-training-stack.md)
- [ADR-0009: Training Run lifecycle, Dataset Snapshots and LoRA Adapter publication](../docs/adr/0009-training-run-lifecycle-and-adapter-publication.md)

## Architecture

This package translates a **Training Dataset** (directory or ZIP archive containing 1024x1024 Ground-layer Map Sections captioned with the Trigger Phrase `dungeonforge battlemap`) plus a **Training Recipe** into a validated **Ostris `ai-toolkit`** job configuration (`extension` job with `diffusion_trainer`).

It enforces:
- **Base Model**: `black-forest-labs/flux-3-image-klein-base` (`arch: "flux3"`)
- **Network**: LoRA `linear: 32`, `linear_alpha: 32` (rank 32 / alpha 32)
- **Optimizer**: `adamw` with `betas: [0.9, 0.95]`, learning rate `1e-4` (`0.0001`), `warmup_steps: 200`, `lr_scheduler: "linear"`
- **Training params**: `batch_size: 1`, `dtype: "bf16"` (bfloat16), resolution `1024x1024`, checkpoint `save_every: 250` steps, `trigger_word: "dungeonforge battlemap"`
- **Sampling**: 1024x1024 preview generation every 250 steps matching checkpoints
- **Run Policies**: Expressed as pure data:
  - `calibration`: 1,500-step smoke validation run
  - `full`: 4,000-step production fine-tuning run
- **Dataset Validation**: Strict pre-flight validation requiring non-empty datasets, companion `.png` + `.txt` pairs, and the Trigger Phrase at the caption head.

## CLI Usage

```bash
# Validate a dataset archive
python3 -m dungeonforge_training /path/to/dataset.zip --validate-only

# Emit YAML configuration for calibration run
python3 -m dungeonforge_training /path/to/dataset.zip --policy calibration > job.yaml

# Emit JSON configuration for full run
python3 -m dungeonforge_training /path/to/dataset_dir --policy full --format json -o job.json

# Using a custom Training Recipe JSON
python3 -m dungeonforge_training /path/to/dataset.zip --recipe recipe.json -o job.yaml
```

## Running Tests

Zero external network access, zero GPU, and zero credentials required:

```bash
# Using pytest
pytest training/tests

# Using unittest
python3 -m unittest discover -s training/tests -t training
```
