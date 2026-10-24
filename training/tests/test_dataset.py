"""Unit tests for dataset inspection and pre-flight validation (Criterion 4)."""

from pathlib import Path
import tempfile
import unittest
import zipfile

from dungeonforge_training.constants import DEFAULT_TRIGGER_PHRASE
from dungeonforge_training.dataset import (
    extract_dataset_if_archive,
    validate_dataset,
)
from dungeonforge_training.errors import DatasetValidationError


class TestDatasetValidation(unittest.TestCase):
    def _create_sample_dir(
        self,
        base_dir: Path,
        samples: list[tuple[str, str]],
    ) -> Path:
        """Helper to create a folder with specified (image_filename, caption_content) pairs."""
        for img_name, caption in samples:
            img_path = base_dir / img_name
            # Write a dummy byte for PNG
            img_path.write_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR...")
            if caption is not None:
                txt_path = base_dir / Path(img_name).with_suffix(".txt").name
                txt_path.write_text(caption, encoding="utf-8")
        return base_dir

    def _create_sample_zip(
        self,
        zip_path: Path,
        samples: list[tuple[str, str]],
        extra_entries: list[tuple[str, bytes]] | None = None,
    ) -> Path:
        """Helper to create a ZIP archive with specified (image_filename, caption_content) pairs."""
        with zipfile.ZipFile(zip_path, "w") as zf:
            for img_name, caption in samples:
                zf.writestr(img_name, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR...")
                if caption is not None:
                    txt_name = str(Path(img_name).with_suffix(".txt"))
                    zf.writestr(txt_name, caption.encode("utf-8"))
            if extra_entries:
                for entry_name, data in extra_entries:
                    zf.writestr(entry_name, data)
        return zip_path

    # Directory validation tests

    def test_valid_directory_dataset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            self._create_sample_dir(
                dir_path,
                [
                    ("sample_01.png", f"{DEFAULT_TRIGGER_PHRASE}, stone dungeon corridor"),
                    ("sample_02.png", f"{DEFAULT_TRIGGER_PHRASE}, cavern pool with glowing moss"),
                ],
            )
            info = validate_dataset(dir_path)
            self.assertFalse(info.is_archive)
            self.assertEqual(info.sample_count, 2)
            self.assertEqual(len(info.samples), 2)
            self.assertEqual(info.trigger_phrase, DEFAULT_TRIGGER_PHRASE)

    def test_directory_empty_dataset_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            empty_dir = Path(tmp_dir)
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(empty_dir)
            self.assertEqual(ctx.exception.code, "empty_dataset")
            self.assertIn("zero .png sample images", str(ctx.exception))

    def test_directory_missing_caption_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            # sample_01 has caption, sample_02 is missing .txt
            self._create_sample_dir(
                dir_path,
                [
                    ("sample_01.png", f"{DEFAULT_TRIGGER_PHRASE}, stone corridor"),
                    ("sample_02.png", None),
                ],
            )
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(dir_path)
            self.assertEqual(ctx.exception.code, "missing_caption")
            self.assertEqual(ctx.exception.sample, "sample_02.png")
            self.assertIn("sample_02.png", str(ctx.exception))
            self.assertIn("missing companion caption file", str(ctx.exception))

    def test_directory_missing_trigger_phrase_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            self._create_sample_dir(
                dir_path,
                [
                    ("sample_01.png", f"{DEFAULT_TRIGGER_PHRASE}, stone corridor"),
                    ("sample_02.png", "legacy battlemap, tavern room with wooden tables"),
                ],
            )
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(dir_path)
            self.assertEqual(ctx.exception.code, "missing_trigger_phrase")
            self.assertEqual(ctx.exception.sample, "sample_02.txt")
            self.assertIn("sample_02.txt", str(ctx.exception))
            self.assertIn(DEFAULT_TRIGGER_PHRASE, str(ctx.exception))

    def test_directory_empty_caption_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            self._create_sample_dir(
                dir_path,
                [
                    ("sample_01.png", "   "),  # Whitespace only
                ],
            )
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(dir_path)
            self.assertEqual(ctx.exception.code, "missing_trigger_phrase")
            self.assertEqual(ctx.exception.sample, "sample_01.txt")

    def test_custom_trigger_phrase_validation(self) -> None:
        custom_phrase = "dungeon master battlemap"
        with tempfile.TemporaryDirectory() as tmp_dir:
            dir_path = Path(tmp_dir)
            self._create_sample_dir(
                dir_path,
                [("sample_01.png", f"{custom_phrase}, ancient crypt")],
            )
            # Default trigger phrase should fail
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(dir_path)
            self.assertEqual(ctx.exception.code, "missing_trigger_phrase")

            # Custom trigger phrase succeeds
            info = validate_dataset(dir_path, trigger_phrase=custom_phrase)
            self.assertEqual(info.sample_count, 1)

    # ZIP archive validation tests

    def test_valid_zip_dataset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            zip_path = Path(tmp_dir) / "dataset.zip"
            self._create_sample_zip(
                zip_path,
                [
                    ("sections/sec_01.png", f"{DEFAULT_TRIGGER_PHRASE}, dungeon entrance"),
                    ("sections/sec_02.png", f"{DEFAULT_TRIGGER_PHRASE}, grand library"),
                ],
            )
            info = validate_dataset(zip_path)
            self.assertTrue(info.is_archive)
            self.assertEqual(info.sample_count, 2)

    def test_zip_empty_dataset_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            zip_path = Path(tmp_dir) / "empty.zip"
            with zipfile.ZipFile(zip_path, "w") as zf:
                zf.writestr("notes.txt", "just a text file")
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(zip_path)
            self.assertEqual(ctx.exception.code, "empty_dataset")

    def test_zip_missing_caption_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            zip_path = Path(tmp_dir) / "dataset.zip"
            self._create_sample_zip(
                zip_path,
                [
                    ("sec_01.png", f"{DEFAULT_TRIGGER_PHRASE}, dungeon entrance"),
                    ("sec_02.png", None),
                ],
            )
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(zip_path)
            self.assertEqual(ctx.exception.code, "missing_caption")
            self.assertEqual(ctx.exception.sample, "sec_02.png")

    def test_zip_missing_trigger_phrase_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            zip_path = Path(tmp_dir) / "dataset.zip"
            self._create_sample_zip(
                zip_path,
                [
                    ("sec_01.png", "ttrpg battlemap, old caption prefix"),
                ],
            )
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(zip_path)
            self.assertEqual(ctx.exception.code, "missing_trigger_phrase")
            self.assertEqual(ctx.exception.sample, "sec_01.txt")

    def test_zip_ignores_macosx_and_dotfiles(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            zip_path = Path(tmp_dir) / "dataset.zip"
            extra = [
                ("__MACOSX/._sec_01.png", b"metadata"),
                (".DS_Store", b"noise"),
                ("sections/.hidden.png", b"ignored"),
            ]
            self._create_sample_zip(
                zip_path,
                [
                    ("sections/sec_01.png", f"{DEFAULT_TRIGGER_PHRASE}, valid section"),
                ],
                extra_entries=extra,
            )
            info = validate_dataset(zip_path)
            self.assertEqual(info.sample_count, 1)

    def test_dataset_not_found(self) -> None:
        with self.assertRaises(DatasetValidationError) as ctx:
            validate_dataset("/path/that/does/not/exist/dataset.zip")
        self.assertEqual(ctx.exception.code, "dataset_not_found")

    def test_invalid_archive_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            bad_zip = Path(tmp_dir) / "not_a_zip.zip"
            bad_zip.write_text("corrupted bytes")
            with self.assertRaises(DatasetValidationError) as ctx:
                validate_dataset(bad_zip)
            self.assertEqual(ctx.exception.code, "invalid_archive")

    def test_extract_dataset_if_archive(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            zip_path = Path(tmp_dir) / "archive.zip"
            self._create_sample_zip(
                zip_path,
                [("sample.png", f"{DEFAULT_TRIGGER_PHRASE}, room")],
            )
            extract_target = Path(tmp_dir) / "extracted"
            res = extract_dataset_if_archive(zip_path, extract_target)
            self.assertEqual(res, extract_target)
            self.assertTrue((extract_target / "sample.png").exists())
            self.assertTrue((extract_target / "sample.txt").exists())

            # For directory, returns path untouched
            dir_res = extract_dataset_if_archive(extract_target, Path(tmp_dir) / "other")
            self.assertEqual(dir_res, extract_target)


if __name__ == "__main__":
    unittest.main()
