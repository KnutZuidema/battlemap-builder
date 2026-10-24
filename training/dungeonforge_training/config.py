"""ai-toolkit job configuration builder for Dungeon Forge LoRA fine-tuning.

Transforms a Training Dataset and Training Recipe into a validated
ai-toolkit YAML/JSON job configuration conforming to ADR-0008 and ADR-0009.
"""

import json
from pathlib import Path
from typing import Any, Mapping, Optional

try:
    import yaml

    HAS_YAML = True
except ImportError:
    HAS_YAML = False

from dungeonforge_training.dataset import validate_dataset
from dungeonforge_training.errors import DatasetValidationError, TrainingError
from dungeonforge_training.recipe import TrainingRecipe


def build_aitoolkit_config(
    dataset_path: str | Path,
    recipe: Optional[TrainingRecipe | Mapping[str, Any]] = None,
    policy: str = "full",
    job_name: Optional[str] = None,
    output_dir: Optional[str] = None,
    device: str = "cuda:0",
    dataset_folder_override: Optional[str] = None,
    validate: bool = True,
) -> dict[str, Any]:
    """Build the ai-toolkit job configuration dictionary.

    Args:
        dataset_path: Path to dataset directory or ZIP archive.
        recipe: TrainingRecipe instance or dict. Defaults to from_policy(policy).
        policy: Policy name if recipe is omitted ('calibration' or 'full').
        job_name: Name of the training job.
        output_dir: Root directory for checkpoints and sample outputs.
        device: Target execution device ('cuda:0' by default).
        dataset_folder_override: Folder path written into datasets config (useful when
            extracting a ZIP archive onto worker local disk).
        validate: Whether to validate dataset before emitting config.

    Returns:
        Validated ai-toolkit job configuration dictionary.

    Raises:
        DatasetValidationError: If dataset validation fails.
        RecipeValidationError: If recipe validation fails.
    """
    # Resolve recipe
    if recipe is None:
        active_recipe = TrainingRecipe.from_policy(policy)
    elif isinstance(recipe, TrainingRecipe):
        active_recipe = recipe
    elif isinstance(recipe, Mapping):
        active_recipe = TrainingRecipe.from_dict(recipe)
    else:
        raise TrainingError("invalid_recipe_type", f"Unsupported recipe type: {type(recipe)}")

    # Validate dataset if requested
    if validate:
        validate_dataset(dataset_path, trigger_phrase=active_recipe.trigger_phrase)

    # Determine job name and output directory
    resolved_job_name = (
        job_name
        or active_recipe.job_name
        or f"dungeonforge-flux3-{active_recipe.run_policy}"
    )
    resolved_output_dir = output_dir or active_recipe.output_dir or "output"

    # Determine folder path to report to ai-toolkit
    if dataset_folder_override:
        resolved_dataset_folder = str(dataset_folder_override)
    else:
        resolved_dataset_folder = str(Path(dataset_path).resolve())

    # Build ai-toolkit configuration dictionary conforming to ADR-0008 & vendor FLUX 3 stack
    aitoolkit_config: dict[str, Any] = {
        "job": "extension",
        "config": {
            "name": resolved_job_name,
            "process": [
                {
                    "type": "diffusion_trainer",
                    "training_folder": resolved_output_dir,
                    "device": device,
                    "trigger_word": active_recipe.trigger_phrase,
                    "network": {
                        "type": "lora",
                        "linear": active_recipe.linear,
                        "linear_alpha": active_recipe.linear_alpha,
                    },
                    "save": {
                        "dtype": active_recipe.dtype,
                        "save_every": active_recipe.save_every,
                        "max_step_saves_to_keep": max(
                            1, active_recipe.steps // active_recipe.save_every
                        ),
                    },
                    "datasets": [
                        {
                            "folder_path": resolved_dataset_folder,
                            "caption_ext": "txt",
                            "caption_dropout_rate": 0.05,
                            "shuffle_tokens": False,
                            "cache_latents_to_disk": True,
                            "resolution": [
                                active_recipe.resolution,
                                active_recipe.resolution,
                            ],
                        }
                    ],
                    "train": {
                        "batch_size": active_recipe.batch_size,
                        "steps": active_recipe.steps,
                        "gradient_accumulation_steps": 1,
                        "train_unet": True,
                        "train_text_encoder": False,
                        "gradient_checkpointing": True,
                        "noise_scheduler": "flowmatch",
                        "optimizer": active_recipe.optimizer,
                        "lr": active_recipe.learning_rate,
                        "optimizer_params": {
                            "betas": list(active_recipe.betas),
                        },
                        "lr_scheduler": active_recipe.lr_scheduler,
                        "warmup_steps": active_recipe.warmup_steps,
                        "dtype": active_recipe.dtype,
                    },
                    "model": {
                        "name_or_path": active_recipe.base_model,
                        "arch": active_recipe.arch,
                        "is_flux": True,
                        "quantize": False,
                    },
                    "sample": {
                        "sampler": "flowmatch",
                        "sample_every": active_recipe.sample_every,
                        "width": active_recipe.resolution,
                        "height": active_recipe.resolution,
                        "prompts": active_recipe.get_sample_prompts(),
                        "neg": "",
                        "seed": 42,
                        "walk_seed": True,
                        "guidance_scale": 3.5,
                        "sample_steps": 28,
                    },
                }
            ],
        },
    }

    return aitoolkit_config


def emit_config(
    dataset_path: str | Path,
    recipe: Optional[TrainingRecipe | Mapping[str, Any]] = None,
    policy: str = "full",
    format: str = "yaml",
    job_name: Optional[str] = None,
    output_dir: Optional[str] = None,
    device: str = "cuda:0",
    dataset_folder_override: Optional[str] = None,
    validate: bool = True,
) -> str:
    """Emit the serialized ai-toolkit configuration string (YAML or JSON).

    Args:
        dataset_path: Path to dataset directory or ZIP archive.
        recipe: TrainingRecipe instance or dictionary.
        policy: Policy name if recipe is omitted ('calibration' or 'full').
        format: Output format ('yaml' or 'json', case-insensitive).
        job_name: Optional job name.
        output_dir: Optional output directory.
        device: Hardware device.
        dataset_folder_override: Dataset folder override in emitted config.
        validate: Whether to validate dataset before emitting config.

    Returns:
        Serialized configuration string.

    Raises:
        DatasetValidationError: When dataset validation fails.
        RecipeValidationError: When recipe parameters fail validation.
        TrainingError: If YAML serialization is requested but PyYAML is unavailable.
    """
    config_dict = build_aitoolkit_config(
        dataset_path=dataset_path,
        recipe=recipe,
        policy=policy,
        job_name=job_name,
        output_dir=output_dir,
        device=device,
        dataset_folder_override=dataset_folder_override,
        validate=validate,
    )

    fmt = format.strip().lower()
    if fmt == "yaml":
        if not HAS_YAML:
            raise TrainingError(
                "yaml_unavailable",
                "PyYAML is not installed; install pyyaml or request format='json'",
            )
        return yaml.dump(config_dict, sort_keys=False, default_flow_style=False)
    elif fmt == "json":
        return json.dumps(config_dict, indent=2)
    else:
        raise TrainingError(
            "invalid_format",
            f"Unsupported configuration format '{format}'. Supported formats: 'yaml', 'json'",
        )
