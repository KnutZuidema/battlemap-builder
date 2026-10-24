"""Run policy definitions for Dungeon Forge LoRA fine-tuning.

Policies are represented strictly as data instances rather than separate code paths,
distinguishing the 1,500-step calibration run from the 4,000-step full run.
"""

from dataclasses import asdict, dataclass
from typing import Any, Mapping

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
    DEFAULT_SAVE_EVERY,
    DEFAULT_TRIGGER_PHRASE,
    DEFAULT_WARMUP_STEPS,
    FULL_STEPS,
)
from dungeonforge_training.errors import RecipeValidationError


@dataclass(frozen=True)
class RunPolicy:
    """Immutable configuration policy for a Training Run.

    All properties are explicit data parameters conforming to ADR-0008.
    """

    name: str
    steps: int
    description: str = ""
    save_every: int = DEFAULT_SAVE_EVERY
    sample_every: int = DEFAULT_SAMPLE_EVERY
    warmup_steps: int = DEFAULT_WARMUP_STEPS
    learning_rate: float = DEFAULT_LEARNING_RATE
    betas: tuple[float, float] = DEFAULT_BETAS
    lr_scheduler: str = DEFAULT_LR_SCHEDULER
    optimizer: str = DEFAULT_OPTIMIZER
    linear: int = DEFAULT_LORA_RANK
    linear_alpha: int = DEFAULT_LORA_ALPHA
    batch_size: int = DEFAULT_BATCH_SIZE
    dtype: str = DEFAULT_DTYPE
    resolution: int = DEFAULT_RESOLUTION
    base_model: str = DEFAULT_BASE_MODEL
    arch: str = DEFAULT_ARCH
    trigger_phrase: str = DEFAULT_TRIGGER_PHRASE

    def to_dict(self) -> dict[str, Any]:
        """Convert policy to a dictionary representation."""
        return asdict(self)


# Predefined run policies as immutable data instances
RUN_POLICIES: dict[str, RunPolicy] = {
    "calibration": RunPolicy(
        name="calibration",
        steps=CALIBRATION_STEPS,
        description=(
            "1,500-step calibration run (~$3.30 on Modal L40S) to validate dataset "
            "packaging, Modal plumbing, and the serving path before the full run."
        ),
    ),
    "full": RunPolicy(
        name="full",
        steps=FULL_STEPS,
        description=(
            "4,000-step full production fine-tuning run (~$8.78 on Modal L40S) "
            "matching the vendor recommended baseline recipe."
        ),
    ),
}


def get_run_policy(name: str) -> RunPolicy:
    """Look up a run policy by name.

    Args:
        name: Name of the policy ('calibration' or 'full', case-insensitive).

    Returns:
        The matched RunPolicy data instance.

    Raises:
        RecipeValidationError: If the policy name is unknown.
    """
    key = name.strip().lower()
    if key not in RUN_POLICIES:
        available = ", ".join(repr(k) for k in sorted(RUN_POLICIES.keys()))
        raise RecipeValidationError(
            code="unknown_run_policy",
            message=f"Unknown run policy '{name}'. Available policies: {available}",
        )
    return RUN_POLICIES[key]


def register_run_policy(policy: RunPolicy) -> None:
    """Register or override a run policy in the data table."""
    RUN_POLICIES[policy.name.lower()] = policy
