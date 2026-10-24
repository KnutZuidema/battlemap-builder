"""Unit tests for TrainingRecipe representation and validation (ADR-0008, ADR-0009)."""

import json
from pathlib import Path
import tempfile
import unittest

from dungeonforge_training.constants import (
    CALIBRATION_STEPS,
    DEFAULT_ARCH,
    DEFAULT_BASE_MODEL,
    DEFAULT_SAMPLE_PROMPTS,
    DEFAULT_TRIGGER_PHRASE,
    FULL_STEPS,
)
from dungeonforge_training.errors import RecipeValidationError
from dungeonforge_training.recipe import TrainingRecipe


class TestTrainingRecipe(unittest.TestCase):
    def test_default_recipe(self) -> None:
        recipe = TrainingRecipe()
        self.assertEqual(recipe.run_policy, "full")
        self.assertEqual(recipe.steps, FULL_STEPS)
        self.assertEqual(recipe.base_model, DEFAULT_BASE_MODEL)
        self.assertEqual(recipe.arch, DEFAULT_ARCH)
        self.assertEqual(recipe.linear, 32)
        self.assertEqual(recipe.linear_alpha, 32)
        self.assertEqual(recipe.optimizer, "adamw")
        self.assertEqual(recipe.learning_rate, 1e-4)
        self.assertEqual(recipe.betas, [0.9, 0.95])
        self.assertEqual(recipe.warmup_steps, 200)
        self.assertEqual(recipe.lr_scheduler, "linear")
        self.assertEqual(recipe.batch_size, 1)
        self.assertEqual(recipe.dtype, "bf16")
        self.assertEqual(recipe.resolution, 1024)
        self.assertEqual(recipe.save_every, 250)
        self.assertEqual(recipe.sample_every, 250)
        self.assertEqual(recipe.trigger_phrase, DEFAULT_TRIGGER_PHRASE)

    def test_from_policy(self) -> None:
        cal = TrainingRecipe.from_policy("calibration")
        self.assertEqual(cal.run_policy, "calibration")
        self.assertEqual(cal.steps, CALIBRATION_STEPS)
        self.assertEqual(cal.steps, 1500)
        self.assertEqual(cal.save_every, 250)
        self.assertEqual(cal.sample_every, 250)

        full = TrainingRecipe.from_policy("full")
        self.assertEqual(full.run_policy, "full")
        self.assertEqual(full.steps, FULL_STEPS)
        self.assertEqual(full.steps, 4000)

    def test_from_policy_with_overrides(self) -> None:
        custom = TrainingRecipe.from_policy(
            "calibration",
            steps=1800,
            trigger_phrase="dungeon room",
            learning_rate=5e-5,
        )
        self.assertEqual(custom.steps, 1800)
        self.assertEqual(custom.trigger_phrase, "dungeon room")
        self.assertEqual(custom.learning_rate, 5e-5)
        self.assertEqual(custom.linear, 32)

    def test_serialization_roundtrip(self) -> None:
        recipe = TrainingRecipe.from_policy("calibration", job_name="test-run")
        data = recipe.to_dict()
        self.assertEqual(data["run_policy"], "calibration")
        self.assertEqual(data["steps"], 1500)
        self.assertEqual(data["job_name"], "test-run")

        reconstructed = TrainingRecipe.from_dict(data)
        self.assertEqual(reconstructed.run_policy, recipe.run_policy)
        self.assertEqual(reconstructed.steps, recipe.steps)
        self.assertEqual(reconstructed.job_name, recipe.job_name)

        json_str = recipe.to_json()
        from_json = TrainingRecipe.from_json(json_str)
        self.assertEqual(from_json.steps, 1500)
        self.assertEqual(from_json.job_name, "test-run")

    def test_from_json_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            recipe_path = Path(tmp_dir) / "recipe.json"
            payload = {
                "run_policy": "calibration",
                "steps": 1500,
                "trigger_phrase": "custom battlemap",
                "learning_rate": 0.0001,
            }
            recipe_path.write_text(json.dumps(payload), encoding="utf-8")

            recipe = TrainingRecipe.from_json_file(recipe_path)
            self.assertEqual(recipe.run_policy, "calibration")
            self.assertEqual(recipe.steps, 1500)
            self.assertEqual(recipe.trigger_phrase, "custom battlemap")
            self.assertEqual(recipe.linear, 32)
            self.assertEqual(recipe.base_model, DEFAULT_BASE_MODEL)

    def test_sample_prompts_default_and_custom(self) -> None:
        recipe = TrainingRecipe(trigger_phrase="mystic battlemap")
        prompts = recipe.get_sample_prompts()
        self.assertEqual(len(prompts), len(DEFAULT_SAMPLE_PROMPTS))
        for p in prompts:
            self.assertTrue(p.startswith("mystic battlemap"))

        custom_recipe = TrainingRecipe(
            trigger_phrase="mystic battlemap",
            sample_prompts=["mystic battlemap, throne room"],
        )
        self.assertEqual(custom_recipe.get_sample_prompts(), ["mystic battlemap, throne room"])

    def test_validation_rejects_invalid_parameters(self) -> None:
        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(steps=0)
        self.assertEqual(ctx.exception.code, "invalid_steps")

        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(save_every=-10)
        self.assertEqual(ctx.exception.code, "invalid_save_every")

        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(learning_rate=-0.01)
        self.assertEqual(ctx.exception.code, "invalid_learning_rate")

        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(linear=0)
        self.assertEqual(ctx.exception.code, "invalid_rank")

        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(trigger_phrase="  ")
        self.assertEqual(ctx.exception.code, "empty_trigger_phrase")

        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(base_model="")
        self.assertEqual(ctx.exception.code, "empty_base_model")

        with self.assertRaises(RecipeValidationError) as ctx:
            TrainingRecipe(betas=[0.9])  # Need 2 betas
        self.assertEqual(ctx.exception.code, "invalid_betas")


if __name__ == "__main__":
    unittest.main()
