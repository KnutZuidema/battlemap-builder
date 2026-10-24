"""Dungeon Forge LoRA fine-tuning and ai-toolkit configuration package.

Standardized per ADR-0008 (FLUX 3 Image Klein LoRA training stack)
and ADR-0009 (Training Run lifecycle, Dataset Snapshots, and LoRA Adapter publication).
"""

from dungeonforge_training.config import (
    build_aitoolkit_config,
    emit_config,
)
from dungeonforge_training.constants import (
    CALIBRATION_STEPS,
    DEFAULT_ARCH,
    DEFAULT_BASE_MODEL,
    DEFAULT_BATCH_SIZE,
    DEFAULT_BETAS,
    DEFAULT_DTYPE,
    DEFAULT_LEARNING_RATE,
    DEFAULT_LORA_ALPHA,
    DEFAULT_LORA_RANK,
    DEFAULT_LR_SCHEDULER,
    DEFAULT_OPTIMIZER,
    DEFAULT_RESOLUTION,
    DEFAULT_SAMPLE_EVERY,
    DEFAULT_SAMPLE_PROMPTS,
    DEFAULT_SAVE_EVERY,
    DEFAULT_TRIGGER_PHRASE,
    DEFAULT_WARMUP_STEPS,
    FULL_STEPS,
)
from dungeonforge_training.dataset import (
    DatasetInfo,
    DatasetSample,
    extract_dataset_if_archive,
    validate_dataset,
)
from dungeonforge_training.errors import (
    DatasetValidationError,
    RecipeValidationError,
    TrainingError,
)
from dungeonforge_training.policy import (
    RUN_POLICIES,
    RunPolicy,
    get_run_policy,
    register_run_policy,
)
from dungeonforge_training.recipe import TrainingRecipe

__version__ = "0.1.0"

__all__ = [
    # Config generation
    "build_aitoolkit_config",
    "emit_config",
    # Dataset inspection & validation
    "validate_dataset",
    "extract_dataset_if_archive",
    "DatasetInfo",
    "DatasetSample",
    # Recipe & Policy
    "TrainingRecipe",
    "RunPolicy",
    "RUN_POLICIES",
    "get_run_policy",
    "register_run_policy",
    # Errors
    "TrainingError",
    "DatasetValidationError",
    "RecipeValidationError",
    # Constants
    "DEFAULT_BASE_MODEL",
    "DEFAULT_ARCH",
    "DEFAULT_TRIGGER_PHRASE",
    "DEFAULT_LORA_RANK",
    "DEFAULT_LORA_ALPHA",
    "DEFAULT_OPTIMIZER",
    "DEFAULT_BETAS",
    "DEFAULT_LEARNING_RATE",
    "DEFAULT_WARMUP_STEPS",
    "DEFAULT_LR_SCHEDULER",
    "DEFAULT_BATCH_SIZE",
    "DEFAULT_DTYPE",
    "DEFAULT_RESOLUTION",
    "DEFAULT_SAVE_EVERY",
    "DEFAULT_SAMPLE_EVERY",
    "CALIBRATION_STEPS",
    "FULL_STEPS",
    "DEFAULT_SAMPLE_PROMPTS",
]
