"""Constants for Dungeon Forge LoRA fine-tuning and ai-toolkit job generation.

Standardized per ADR-0008 (FLUX 3 Image Klein LoRA training stack)
and ADR-0009 (Training Run lifecycle, Dataset Snapshots, and LoRA Adapter publication).
"""

from typing import Final

# Base Model & Architecture (ADR-0008)
DEFAULT_BASE_MODEL: Final[str] = "black-forest-labs/flux-3-image-klein-base"
DEFAULT_ARCH: Final[str] = "flux3"

# Trigger Phrase (ADR-0008, ADR-0009)
DEFAULT_TRIGGER_PHRASE: Final[str] = "dungeonforge battlemap"

# LoRA Network Architecture (ADR-0008: rank 32 / alpha 32)
DEFAULT_LORA_RANK: Final[int] = 32
DEFAULT_LORA_ALPHA: Final[int] = 32

# Optimizer & Schedule (ADR-0008: AdamW betas 0.9, 0.95 at lr 1e-4, 200 warmup steps, linear decay)
DEFAULT_OPTIMIZER: Final[str] = "adamw"
DEFAULT_BETAS: Final[tuple[float, float]] = (0.9, 0.95)
DEFAULT_LEARNING_RATE: Final[float] = 1e-4
DEFAULT_WARMUP_STEPS: Final[int] = 200
DEFAULT_LR_SCHEDULER: Final[str] = "linear"

# Training Parameters (ADR-0008: batch size 1, bfloat16, 1024x1024 native resolution)
DEFAULT_BATCH_SIZE: Final[int] = 1
DEFAULT_DTYPE: Final[str] = "bf16"
DEFAULT_RESOLUTION: Final[int] = 1024

# Checkpointing & Sampling Frequency (ADR-0008, ADR-0009: checkpoint & sample every 250 steps)
DEFAULT_SAVE_EVERY: Final[int] = 250
DEFAULT_SAMPLE_EVERY: Final[int] = 250

# Run Policy Step Counts (ADR-0008)
CALIBRATION_STEPS: Final[int] = 1500
FULL_STEPS: Final[int] = 4000

# Default sample prompts evaluated at each checkpoint to verify convergence and catch degradation
DEFAULT_SAMPLE_PROMPTS: Final[list[str]] = [
    "{trigger_phrase}, stone dungeon corridor with torchlight and iron portcullis",
    "{trigger_phrase}, tavern common room with wooden tables and stone hearth",
    "{trigger_phrase}, subterranean cavern with subterranean river and mossy rocks",
    "{trigger_phrase}, ancient crypt with sarcophagi and ruined stone pillars",
]
