"""Unit tests for run policy data definitions (ADR-0008, Criterion 3)."""

import unittest

from dungeonforge_training.constants import CALIBRATION_STEPS, FULL_STEPS
from dungeonforge_training.errors import RecipeValidationError
from dungeonforge_training.policy import (
    RUN_POLICIES,
    RunPolicy,
    get_run_policy,
    register_run_policy,
)


class TestRunPolicy(unittest.TestCase):
    def test_run_policies_defined_as_data(self) -> None:
        """Run policy distinguishes calibration from full run as data instances."""
        self.assertIn("calibration", RUN_POLICIES)
        self.assertIn("full", RUN_POLICIES)

        cal = RUN_POLICIES["calibration"]
        self.assertIsInstance(cal, RunPolicy)
        self.assertEqual(cal.name, "calibration")
        self.assertEqual(cal.steps, CALIBRATION_STEPS)
        self.assertEqual(cal.steps, 1500)
        self.assertEqual(cal.save_every, 250)
        self.assertEqual(cal.sample_every, 250)
        self.assertEqual(cal.warmup_steps, 200)
        self.assertEqual(cal.learning_rate, 1e-4)
        self.assertEqual(cal.betas, (0.9, 0.95))
        self.assertEqual(cal.lr_scheduler, "linear")
        self.assertEqual(cal.optimizer, "adamw")
        self.assertEqual(cal.linear, 32)
        self.assertEqual(cal.linear_alpha, 32)
        self.assertEqual(cal.batch_size, 1)
        self.assertEqual(cal.dtype, "bf16")
        self.assertEqual(cal.resolution, 1024)
        self.assertEqual(cal.base_model, "black-forest-labs/flux-3-image-klein-base")
        self.assertEqual(cal.arch, "flux3")
        self.assertEqual(cal.trigger_phrase, "dungeonforge battlemap")

        full = RUN_POLICIES["full"]
        self.assertIsInstance(full, RunPolicy)
        self.assertEqual(full.name, "full")
        self.assertEqual(full.steps, FULL_STEPS)
        self.assertEqual(full.steps, 4000)
        self.assertEqual(full.save_every, 250)
        self.assertEqual(full.sample_every, 250)

        # Confirm shared hyperparameter parity between policies
        self.assertEqual(cal.linear, full.linear)
        self.assertEqual(cal.linear_alpha, full.linear_alpha)
        self.assertEqual(cal.learning_rate, full.learning_rate)
        self.assertEqual(cal.betas, full.betas)
        self.assertEqual(cal.base_model, full.base_model)
        self.assertEqual(cal.arch, full.arch)
        self.assertEqual(cal.trigger_phrase, full.trigger_phrase)

    def test_get_run_policy(self) -> None:
        cal = get_run_policy("calibration")
        self.assertEqual(cal.steps, 1500)

        # Case-insensitivity
        cal_upper = get_run_policy("CALIBRATION")
        self.assertEqual(cal_upper.steps, 1500)

        full = get_run_policy("full")
        self.assertEqual(full.steps, 4000)

    def test_unknown_policy_raises_descriptive_error(self) -> None:
        with self.assertRaises(RecipeValidationError) as ctx:
            get_run_policy("nonexistent_policy")
        self.assertEqual(ctx.exception.code, "unknown_run_policy")
        self.assertIn("nonexistent_policy", str(ctx.exception))

    def test_register_run_policy(self) -> None:
        custom = RunPolicy(name="custom_smoke", steps=500)
        register_run_policy(custom)
        self.assertEqual(get_run_policy("custom_smoke").steps, 500)

    def test_to_dict(self) -> None:
        cal_dict = RUN_POLICIES["calibration"].to_dict()
        self.assertIsInstance(cal_dict, dict)
        self.assertEqual(cal_dict["name"], "calibration")
        self.assertEqual(cal_dict["steps"], 1500)
        self.assertEqual(cal_dict["save_every"], 250)


if __name__ == "__main__":
    unittest.main()
