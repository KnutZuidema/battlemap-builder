"""Unit tests for ai-toolkit configuration generator (Criterion 1, 2, 3, 4)."""

import json
from pathlib import Path
import tempfile
import unittest
import yaml

from dungeonforge_training.config import build_aitoolkit_config, emit_config
from dungeonforge_training.constants import (
    CALIBRATION_STEPS,
    DEFAULT_ARCH,
    DEFAULT_BASE_MODEL,
    DEFAULT_TRIGGER_PHRASE,
    FULL_STEPS,
)
from dungeonforge_training.errors import DatasetValidationError
from dungeonforge_training.recipe import TrainingRecipe


class TestConfigGeneration(unittest.TestCase):
    def _create_valid_dataset(self, base_dir: Path, count: int = 2) -> Path:
        for i in range(1, count + 1):
            png = base_dir / f"sec_{i:02d}.png"
            png.write_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR...")
            txt = base_dir / f"sec_{i:02d}.txt"
            txt.write_text(f"{DEFAULT_TRIGGER_PHRASE}, map section {i}", encoding="utf-8")
        return base_dir

    def test_calibration_aitoolkit_configuration_matches_adr0008(self) -> None:
        """Verify calibration ai-toolkit configuration exact parameter parity with ADR-0008."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_valid_dataset(Path(tmp_dir))
            config = build_aitoolkit_config(
                dataset_path=dataset_path,
                policy="calibration",
                job_name="calibration-run-001",
                output_dir="/output/run1",
            )

            # Root structure
            self.assertEqual(config["job"], "extension")
            self.assertIn("config", config)
            cfg = config["config"]
            self.assertEqual(cfg["name"], "calibration-run-001")
            self.assertIn("process", cfg)
            self.assertEqual(len(cfg["process"]), 1)

            proc = cfg["process"][0]
            self.assertEqual(proc["type"], "diffusion_trainer")
            self.assertEqual(proc["training_folder"], "/output/run1")
            self.assertEqual(proc["device"], "cuda:0")
            self.assertEqual(proc["trigger_word"], "dungeonforge battlemap")

            # Network: LoRA linear: 32, linear_alpha: 32 (rank 32 / alpha 32)
            net = proc["network"]
            self.assertEqual(net["type"], "lora")
            self.assertEqual(net["linear"], 32)
            self.assertEqual(net["linear_alpha"], 32)

            # Model: black-forest-labs/flux-3-image-klein-base (arch: flux3)
            model = proc["model"]
            self.assertEqual(model["name_or_path"], "black-forest-labs/flux-3-image-klein-base")
            self.assertEqual(model["arch"], "flux3")
            self.assertTrue(model["is_flux"])
            self.assertFalse(model["quantize"])

            # Train: AdamW betas [0.9, 0.95], lr 1e-4, warmup_steps 200, lr_scheduler linear, batch_size 1, bf16
            train = proc["train"]
            self.assertEqual(train["batch_size"], 1)
            self.assertEqual(train["steps"], 1500)
            self.assertEqual(train["dtype"], "bf16")
            self.assertEqual(train["optimizer"], "adamw")
            self.assertEqual(train["lr"], 0.0001)
            self.assertEqual(train["optimizer_params"]["betas"], [0.9, 0.95])
            self.assertEqual(train["warmup_steps"], 200)
            self.assertEqual(train["lr_scheduler"], "linear")
            self.assertEqual(train["noise_scheduler"], "flowmatch")
            self.assertTrue(train["gradient_checkpointing"])
            self.assertTrue(train["train_unet"])
            self.assertFalse(train["train_text_encoder"])

            # Save: save_every 250, dtype bf16
            save = proc["save"]
            self.assertEqual(save["save_every"], 250)
            self.assertEqual(save["dtype"], "bf16")

            # Datasets: resolution [1024, 1024], native, no resampling
            datasets = proc["datasets"]
            self.assertEqual(len(datasets), 1)
            ds = datasets[0]
            self.assertEqual(ds["folder_path"], str(dataset_path.resolve()))
            self.assertEqual(ds["caption_ext"], "txt")
            self.assertEqual(ds["resolution"], [1024, 1024])
            self.assertTrue(ds["cache_latents_to_disk"])

            # Sample: every 250 steps at 1024x1024
            sample = proc["sample"]
            self.assertEqual(sample["sample_every"], 250)
            self.assertEqual(sample["width"], 1024)
            self.assertEqual(sample["height"], 1024)
            self.assertEqual(sample["sampler"], "flowmatch")
            self.assertGreaterEqual(len(sample["prompts"]), 4)
            for p in sample["prompts"]:
                self.assertTrue(p.startswith("dungeonforge battlemap"))

    def test_full_aitoolkit_configuration_matches_adr0008(self) -> None:
        """Verify full run ai-toolkit configuration uses 4,000 steps."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_valid_dataset(Path(tmp_dir))
            config = build_aitoolkit_config(
                dataset_path=dataset_path,
                policy="full",
            )
            proc = config["config"]["process"][0]
            self.assertEqual(proc["train"]["steps"], 4000)
            self.assertEqual(proc["save"]["save_every"], 250)
            self.assertEqual(proc["sample"]["sample_every"], 250)

    def test_refuses_to_emit_for_empty_dataset(self) -> None:
        """Fails loud with DatasetValidationError when dataset is empty."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            empty_path = Path(tmp_dir)
            with self.assertRaises(DatasetValidationError) as ctx:
                build_aitoolkit_config(dataset_path=empty_path, policy="full")
            self.assertEqual(ctx.exception.code, "empty_dataset")

    def test_refuses_to_emit_for_missing_caption(self) -> None:
        """Fails loud when a sample lacks companion caption file."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            (dir_path / "map_01.png").write_bytes(b"\x89PNG...")
            # No map_01.txt
            with self.assertRaises(DatasetValidationError) as ctx:
                build_aitoolkit_config(dataset_path=dir_path, policy="calibration")
            self.assertEqual(ctx.exception.code, "missing_caption")
            self.assertEqual(ctx.exception.sample, "map_01.png")

    def test_refuses_to_emit_for_missing_trigger_phrase(self) -> None:
        """Fails loud when caption does not start with trigger phrase."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            (dir_path / "map_01.png").write_bytes(b"\x89PNG...")
            (dir_path / "map_01.txt").write_text("plain map description", encoding="utf-8")
            with self.assertRaises(DatasetValidationError) as ctx:
                build_aitoolkit_config(dataset_path=dir_path, policy="calibration")
            self.assertEqual(ctx.exception.code, "missing_trigger_phrase")
            self.assertEqual(ctx.exception.sample, "map_01.txt")

    def test_emit_config_yaml_serialization(self) -> None:
        """Verify YAML string output is valid and decodable."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_valid_dataset(Path(tmp_dir))
            yaml_str = emit_config(
                dataset_path=dataset_path,
                policy="calibration",
                format="yaml",
            )
            self.assertIn("job: extension", yaml_str)
            self.assertIn("linear: 32", yaml_str)
            self.assertIn("linear_alpha: 32", yaml_str)
            self.assertIn("steps: 1500", yaml_str)

            # Roundtrip YAML parse
            parsed = yaml.safe_load(yaml_str)
            self.assertEqual(parsed["job"], "extension")
            self.assertEqual(parsed["config"]["process"][0]["train"]["steps"], 1500)

    def test_emit_config_json_serialization(self) -> None:
        """Verify JSON string output is valid and decodable."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_valid_dataset(Path(tmp_dir))
            json_str = emit_config(
                dataset_path=dataset_path,
                policy="full",
                format="json",
            )
            parsed = json.loads(json_str)
            self.assertEqual(parsed["job"], "extension")
            self.assertEqual(parsed["config"]["process"][0]["train"]["steps"], 4000)

    def test_explicit_recipe_customization(self) -> None:
        """Supports customized TrainingRecipe with non-default trigger phrase and step count."""
        recipe = TrainingRecipe.from_policy(
            "full",
            steps=5000,
            trigger_phrase="castle battlemap",
            linear=64,
            linear_alpha=64,
        )
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            (dir_path / "sec_01.png").write_bytes(b"\x89PNG...")
            (dir_path / "sec_01.txt").write_text("castle battlemap, courtyard", encoding="utf-8")

            config = build_aitoolkit_config(
                dataset_path=dir_path,
                recipe=recipe,
            )
            proc = config["config"]["process"][0]
            self.assertEqual(proc["trigger_word"], "castle battlemap")
            self.assertEqual(proc["train"]["steps"], 5000)
            self.assertEqual(proc["network"]["linear"], 64)
            self.assertEqual(proc["network"]["linear_alpha"], 64)


if __name__ == "__main__":
    unittest.main()
