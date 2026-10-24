"""Command-line interface for Dungeon Forge LoRA training configuration."""

import argparse
from pathlib import Path
import sys
from typing import Optional, Sequence

from dungeonforge_training.config import emit_config
from dungeonforge_training.dataset import validate_dataset
from dungeonforge_training.errors import DatasetValidationError, RecipeValidationError, TrainingError
from dungeonforge_training.recipe import TrainingRecipe


def create_parser() -> argparse.ArgumentParser:
    """Create command-line argument parser."""
    parser = argparse.ArgumentParser(
        prog="dungeonforge-training",
        description="Generate validated ai-toolkit job configurations for Dungeon Forge LoRA training.",
    )

    parser.add_argument(
        "dataset_pos",
        nargs="?",
        metavar="DATASET",
        help="Path to Training Dataset directory or ZIP archive.",
    )
    parser.add_argument(
        "-d",
        "--dataset",
        dest="dataset_opt",
        help="Path to Training Dataset directory or ZIP archive (alternative to positional argument).",
    )
    parser.add_argument(
        "-p",
        "--policy",
        choices=["calibration", "full"],
        default="full",
        help="Run policy to select: 'calibration' (1,500 steps) or 'full' (4,000 steps). Default: full.",
    )
    parser.add_argument(
        "-r",
        "--recipe",
        help="Path to an explicit TrainingRecipe JSON file (overrides --policy defaults).",
    )
    parser.add_argument(
        "-f",
        "--format",
        choices=["yaml", "json"],
        default="yaml",
        help="Output serialization format ('yaml' or 'json'). Default: yaml.",
    )
    parser.add_argument(
        "-o",
        "--output",
        help="Output file path. If omitted, prints configuration to stdout.",
    )
    parser.add_argument(
        "--job-name",
        help="Name of the training job. Defaults to dungeonforge-flux3-<policy>.",
    )
    parser.add_argument(
        "--output-dir",
        help="Directory to save checkpoints and evaluation samples. Default: 'output'.",
    )
    parser.add_argument(
        "--trigger-phrase",
        help="Override trigger phrase (default: 'dungeonforge battlemap').",
    )
    parser.add_argument(
        "--steps",
        type=int,
        help="Override total training step count.",
    )
    parser.add_argument(
        "--save-every",
        type=int,
        help="Override checkpoint save frequency in steps (default: 250).",
    )
    parser.add_argument(
        "--sample-every",
        type=int,
        help="Override evaluation sampling frequency in steps (default: 250).",
    )
    parser.add_argument(
        "--dataset-folder-override",
        help="Override dataset folder path in emitted configuration (for remote runner environments).",
    )
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="Validate the dataset without emitting an ai-toolkit configuration.",
    )
    parser.add_argument(
        "--no-validate",
        action="store_true",
        help="Bypass local dataset validation (e.g. when emitting config for a remote URL).",
    )

    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    """CLI entry point for dungeonforge-training.

    Returns:
        0 on success, non-zero on error.
    """
    parser = create_parser()
    args = parser.parse_args(argv)

    dataset_path = args.dataset_opt or args.dataset_pos
    if not dataset_path:
        parser.print_usage(sys.stderr)
        sys.stderr.write("error: Dataset path must be provided as positional argument or via --dataset / -d\n")
        return 1

    try:
        # Load or construct recipe
        overrides: dict[str, object] = {}
        if args.trigger_phrase:
            overrides["trigger_phrase"] = args.trigger_phrase
        if args.steps is not None:
            overrides["steps"] = args.steps
        if args.save_every is not None:
            overrides["save_every"] = args.save_every
        if args.sample_every is not None:
            overrides["sample_every"] = args.sample_every
        if args.job_name:
            overrides["job_name"] = args.job_name
        if args.output_dir:
            overrides["output_dir"] = args.output_dir

        if args.recipe:
            recipe = TrainingRecipe.from_json_file(args.recipe)
            if overrides:
                # Apply CLI overrides onto loaded recipe
                recipe_data = recipe.to_dict()
                recipe_data.update(overrides)
                recipe = TrainingRecipe.from_dict(recipe_data)
        else:
            recipe = TrainingRecipe.from_policy(args.policy, **overrides)

        # Handle --validate-only
        if args.validate_only:
            info = validate_dataset(dataset_path, trigger_phrase=recipe.trigger_phrase)
            sys.stdout.write(
                f"Valid dataset at '{dataset_path}':\n"
                f"  Type: {'ZIP Archive' if info.is_archive else 'Directory'}\n"
                f"  Valid samples: {info.sample_count}\n"
                f"  Trigger phrase: '{info.trigger_phrase}'\n"
            )
            return 0

        # Emit configuration
        config_text = emit_config(
            dataset_path=dataset_path,
            recipe=recipe,
            format=args.format,
            job_name=args.job_name,
            output_dir=args.output_dir,
            dataset_folder_override=args.dataset_folder_override,
            validate=not args.no_validate,
        )

        if args.output:
            out_path = Path(args.output)
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_text(config_text, encoding="utf-8")
        else:
            sys.stdout.write(config_text)
            if not config_text.endswith("\n"):
                sys.stdout.write("\n")

        return 0

    except DatasetValidationError as exc:
        sys.stderr.write(f"Dataset validation failed: {exc}\n")
        return 1
    except (RecipeValidationError, TrainingError) as exc:
        sys.stderr.write(f"Configuration error: {exc}\n")
        return 1
    except Exception as exc:
        sys.stderr.write(f"Unexpected error: {exc}\n")
        return 1


if __name__ == "__main__":
    sys.exit(main())
