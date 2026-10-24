"""Training Recipe representation for Dungeon Forge LoRA fine-tuning.

Per ADR-0008 and ADR-0009, a Training Recipe is the exact Base Model and
hyperparameter configuration a Training Run dispatches, recorded with the run
so it can be replayed field for field.
"""

from dataclasses import asdict, dataclass, field
import json
from pathlib import Path
from typing import Any, Mapping, Optional, Sequence

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
from dungeonforge_training.errors import RecipeValidationError
from dungeonforge_training.policy import RunPolicy, get_run_policy


@dataclass
class TrainingRecipe:
    """Exact hyperparameter and model configuration for a Training Run.

    Conforms to ADR-0008 vendor baseline for FLUX 3 Image Klein LoRA training.
    """

    run_policy: str = "full"
    steps: int = FULL_STEPS
    base_model: str = DEFAULT_BASE_MODEL
    arch: str = DEFAULT_ARCH
    linear: int = DEFAULT_LORA_RANK
    linear_alpha: int = DEFAULT_LORA_ALPHA
    optimizer: str = DEFAULT_OPTIMIZER
    learning_rate: float = DEFAULT_LEARNING_RATE
    betas: list[float] = field(default_factory=lambda: list(DEFAULT_BETAS))
    warmup_steps: int = DEFAULT_WARMUP_STEPS
    lr_scheduler: str = DEFAULT_LR_SCHEDULER
    batch_size: int = DEFAULT_BATCH_SIZE
    dtype: str = DEFAULT_DTYPE
    resolution: int = DEFAULT_RESOLUTION
    save_every: int = DEFAULT_SAVE_EVERY
    sample_every: int = DEFAULT_SAMPLE_EVERY
    trigger_phrase: str = DEFAULT_TRIGGER_PHRASE
    sample_prompts: list[str] = field(default_factory=list)
    job_name: Optional[str] = None
    output_dir: Optional[str] = None
    extra_params: dict[str, Any] = field(default_factory=dict)

    def __post_init__(self) -> None:
        self.validate()

    def validate(self) -> None:
        """Validate recipe parameters, raising RecipeValidationError on defect."""
        if not self.base_model or not self.base_model.strip():
            raise RecipeValidationError("empty_base_model", "base_model must not be empty")

        if not self.arch or not self.arch.strip():
            raise RecipeValidationError("empty_arch", "arch must not be empty")

        if not self.trigger_phrase or not self.trigger_phrase.strip():
            raise RecipeValidationError("empty_trigger_phrase", "trigger_phrase must not be empty")

        if self.steps <= 0:
            raise RecipeValidationError("invalid_steps", f"steps must be > 0, got {self.steps}")

        if self.save_every <= 0:
            raise RecipeValidationError(
                "invalid_save_every", f"save_every must be > 0, got {self.save_every}"
            )

        if self.sample_every <= 0:
            raise RecipeValidationError(
                "invalid_sample_every", f"sample_every must be > 0, got {self.sample_every}"
            )

        if self.learning_rate <= 0:
            raise RecipeValidationError(
                "invalid_learning_rate",
                f"learning_rate must be > 0, got {self.learning_rate}",
            )

        if self.warmup_steps < 0:
            raise RecipeValidationError(
                "invalid_warmup_steps",
                f"warmup_steps must be >= 0, got {self.warmup_steps}",
            )

        if self.linear <= 0:
            raise RecipeValidationError("invalid_rank", f"linear rank must be > 0, got {self.linear}")

        if self.linear_alpha <= 0:
            raise RecipeValidationError(
                "invalid_alpha", f"linear_alpha must be > 0, got {self.linear_alpha}"
            )

        if self.batch_size <= 0:
            raise RecipeValidationError(
                "invalid_batch_size", f"batch_size must be > 0, got {self.batch_size}"
            )

        if self.resolution <= 0:
            raise RecipeValidationError(
                "invalid_resolution", f"resolution must be > 0, got {self.resolution}"
            )

        if len(self.betas) != 2 or not all(0.0 <= b < 1.0 for b in self.betas):
            raise RecipeValidationError(
                "invalid_betas",
                f"betas must be a 2-element sequence of floats in [0, 1), got {self.betas}",
            )

    def get_sample_prompts(self) -> list[str]:
        """Return sample prompts, formatting default prompts with trigger_phrase if not specified."""
        if self.sample_prompts:
            return list(self.sample_prompts)
        return [
            prompt.format(trigger_phrase=self.trigger_phrase)
            for prompt in DEFAULT_SAMPLE_PROMPTS
        ]

    def to_dict(self) -> dict[str, Any]:
        """Serialize recipe to a plain Python dictionary."""
        return asdict(self)

    def to_json(self, indent: int = 2) -> str:
        """Serialize recipe to formatted JSON string."""
        return json.dumps(self.to_dict(), indent=indent)

    @classmethod
    def from_policy(
        cls,
        policy: str | RunPolicy,
        **overrides: Any,
    ) -> "TrainingRecipe":
        """Instantiate a recipe derived from a named RunPolicy or RunPolicy instance with overrides."""
        if isinstance(policy, str):
            pol = get_run_policy(policy)
        else:
            pol = policy

        data: dict[str, Any] = {
            "run_policy": pol.name,
            "steps": pol.steps,
            "base_model": pol.base_model,
            "arch": pol.arch,
            "linear": pol.linear,
            "linear_alpha": pol.linear_alpha,
            "optimizer": pol.optimizer,
            "learning_rate": pol.learning_rate,
            "betas": list(pol.betas),
            "warmup_steps": pol.warmup_steps,
            "lr_scheduler": pol.lr_scheduler,
            "batch_size": pol.batch_size,
            "dtype": pol.dtype,
            "resolution": pol.resolution,
            "save_every": pol.save_every,
            "sample_every": pol.sample_every,
            "trigger_phrase": pol.trigger_phrase,
        }
        data.update(overrides)
        return cls(**data)

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> "TrainingRecipe":
        """Instantiate a recipe from a dictionary (e.g. parsed JSON payload)."""
        clean_data = dict(data)
        # If run_policy is specified and some fields are omitted, seed with policy defaults
        policy_name = clean_data.get("run_policy")
        if policy_name and policy_name in ("calibration", "full"):
            try:
                base_recipe = cls.from_policy(policy_name)
                merged = base_recipe.to_dict()
                merged.update(clean_data)
                clean_data = merged
            except RecipeValidationError:
                pass

        # Handle list/tuple conversion for betas
        if "betas" in clean_data and isinstance(clean_data["betas"], (tuple, list)):
            clean_data["betas"] = [float(b) for b in clean_data["betas"]]

        # Extract known fields
        known_fields = {
            "run_policy",
            "steps",
            "base_model",
            "arch",
            "linear",
            "linear_alpha",
            "optimizer",
            "learning_rate",
            "betas",
            "warmup_steps",
            "lr_scheduler",
            "batch_size",
            "dtype",
            "resolution",
            "save_every",
            "sample_every",
            "trigger_phrase",
            "sample_prompts",
            "job_name",
            "output_dir",
            "extra_params",
        }
        init_kwargs = {k: v for k, v in clean_data.items() if k in known_fields}
        extras = {k: v for k, v in clean_data.items() if k not in known_fields}
        if extras and "extra_params" not in init_kwargs:
            init_kwargs["extra_params"] = extras
        elif extras and isinstance(init_kwargs.get("extra_params"), dict):
            init_kwargs["extra_params"].update(extras)

        return cls(**init_kwargs)

    @classmethod
    def from_json(cls, json_str: str) -> "TrainingRecipe":
        """Instantiate a recipe from a JSON string."""
        try:
            parsed = json.loads(json_str)
        except json.JSONDecodeError as exc:
            raise RecipeValidationError(
                "invalid_json", f"Failed to parse recipe JSON: {exc}"
            ) from exc
        if not isinstance(parsed, dict):
            raise RecipeValidationError("invalid_json", "Recipe JSON root must be an object")
        return cls.from_dict(parsed)

    @classmethod
    def from_json_file(cls, path: str | Path) -> "TrainingRecipe":
        """Load and parse recipe from a JSON file."""
        file_path = Path(path)
        if not file_path.is_file():
            raise RecipeValidationError(
                "file_not_found", f"Recipe file does not exist: {file_path}"
            )
        try:
            content = file_path.read_text(encoding="utf-8")
        except Exception as exc:
            raise RecipeValidationError(
                "read_error", f"Could not read recipe file {file_path}: {exc}"
            ) from exc
        return cls.from_json(content)
