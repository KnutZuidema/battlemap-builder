"""Unit tests for the CLI entry point (Criterion 5)."""

import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import yaml

from dungeonforge_training.cli import main
from dungeonforge_training.constants import DEFAULT_TRIGGER_PHRASE


class TestCLI(unittest.TestCase):
    def _create_sample_dataset(self, folder: Path, count: int = 2) -> Path:
        for i in range(1, count + 1):
            (folder / f"sec_{i}.png").write_bytes(b"\x89PNG...")
            (folder / f"sec_{i}.txt").write_text(
                f"{DEFAULT_TRIGGER_PHRASE}, corridor {i}", encoding="utf-8"
            )
        return folder

    def test_cli_emits_yaml_stdout(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_sample_dataset(Path(tmp_dir))
            out_buf = io.StringIO()
            err_buf = io.StringIO()

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = out_buf, err_buf
                rc = main([str(dataset_path), "--policy", "calibration"])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 0)
            output = out_buf.getvalue()
            self.assertIn("job: extension", output)
            self.assertIn("steps: 1500", output)
            self.assertIn("linear: 32", output)
            parsed = yaml.safe_load(output)
            self.assertEqual(parsed["job"], "extension")

    def test_cli_emits_json_format(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_sample_dataset(Path(tmp_dir))
            out_buf = io.StringIO()
            err_buf = io.StringIO()

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = out_buf, err_buf
                rc = main([str(dataset_path), "--format", "json", "--policy", "full"])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 0)
            output = out_buf.getvalue()
            parsed = json.loads(output)
            self.assertEqual(parsed["config"]["process"][0]["train"]["steps"], 4000)

    def test_cli_writes_to_output_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_sample_dataset(Path(tmp_dir))
            out_file = Path(tmp_dir) / "output" / "config.yaml"

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = io.StringIO(), io.StringIO()
                rc = main([str(dataset_path), "-o", str(out_file)])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 0)
            self.assertTrue(out_file.exists())
            content = out_file.read_text(encoding="utf-8")
            self.assertIn("job: extension", content)

    def test_cli_with_recipe_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_sample_dataset(Path(tmp_dir))
            recipe_file = Path(tmp_dir) / "recipe.json"
            recipe_file.write_text(
                json.dumps(
                    {
                        "run_policy": "calibration",
                        "steps": 1200,
                        "trigger_phrase": "dungeonforge battlemap",
                    }
                ),
                encoding="utf-8",
            )

            out_buf = io.StringIO()
            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = out_buf, io.StringIO()
                rc = main([str(dataset_path), "--recipe", str(recipe_file)])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 0)
            parsed = yaml.safe_load(out_buf.getvalue())
            self.assertEqual(parsed["config"]["process"][0]["train"]["steps"], 1200)

    def test_cli_fails_on_empty_dataset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            empty_path = Path(tmp_dir)
            err_buf = io.StringIO()

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = io.StringIO(), err_buf
                rc = main([str(empty_path)])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 1)
            err_output = err_buf.getvalue()
            self.assertIn("empty_dataset", err_output)

    def test_cli_fails_on_missing_caption(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            bad_dir = Path(tmp_dir)
            (bad_dir / "sample_orphan.png").write_bytes(b"\x89PNG...")
            err_buf = io.StringIO()

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = io.StringIO(), err_buf
                rc = main([str(bad_dir)])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 1)
            err_output = err_buf.getvalue()
            self.assertIn("missing_caption", err_output)
            self.assertIn("sample_orphan.png", err_output)

    def test_cli_fails_on_missing_trigger_phrase(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            bad_dir = Path(tmp_dir)
            (bad_dir / "sample.png").write_bytes(b"\x89PNG...")
            (bad_dir / "sample.txt").write_text("unqualified caption", encoding="utf-8")
            err_buf = io.StringIO()

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = io.StringIO(), err_buf
                rc = main([str(bad_dir)])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 1)
            err_output = err_buf.getvalue()
            self.assertIn("missing_trigger_phrase", err_output)
            self.assertIn("sample.txt", err_output)

    def test_cli_validate_only_flag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_sample_dataset(Path(tmp_dir))
            out_buf = io.StringIO()

            old_stdout, old_stderr = sys.stdout, sys.stderr
            try:
                sys.stdout, sys.stderr = out_buf, io.StringIO()
                rc = main([str(dataset_path), "--validate-only"])
            finally:
                sys.stdout, sys.stderr = old_stdout, old_stderr

            self.assertEqual(rc, 0)
            output = out_buf.getvalue()
            self.assertIn("Valid dataset", output)
            self.assertIn("Valid samples: 2", output)
            # Did not emit full YAML config
            self.assertNotIn("job: extension", output)

    def test_cli_subprocess_invocation(self) -> None:
        """Assert python3 -m dungeonforge_training entrypoint executes successfully in a subshell."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            dataset_path = self._create_sample_dataset(Path(tmp_dir))
            repo_root = Path(__file__).resolve().parent.parent

            cmd = [
                sys.executable,
                "-m",
                "dungeonforge_training",
                str(dataset_path),
                "--policy",
                "calibration",
            ]
            res = subprocess.run(
                cmd,
                cwd=str(repo_root),
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(res.returncode, 0, msg=f"CLI failed: {res.stderr}")
            self.assertIn("job: extension", res.stdout)
            self.assertIn("steps: 1500", res.stdout)


if __name__ == "__main__":
    unittest.main()
