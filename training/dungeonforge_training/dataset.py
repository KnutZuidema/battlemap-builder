"""Training Dataset validation and inspection for Dungeon Forge LoRA fine-tuning.

Per ADR-0008 and ADR-0009:
- A Training Dataset consists of 1024x1024 Ground-layer Map Sections.
- Every sample consists of a companion pair: a .png image and a .txt caption file.
- Every caption file MUST begin with the Trigger Phrase ('dungeonforge battlemap' by default).
- Zero valid sample pairs, missing captions, or missing trigger phrases cause explicit validation errors.
"""

from dataclasses import dataclass
from pathlib import Path
import re
from typing import List, Optional, Sequence
import zipfile

from dungeonforge_training.constants import DEFAULT_TRIGGER_PHRASE
from dungeonforge_training.errors import DatasetValidationError


@dataclass(frozen=True)
class DatasetSample:
    """A validated sample pair within a Training Dataset."""

    image_path: str
    caption_path: str
    caption_text: str


@dataclass(frozen=True)
class DatasetInfo:
    """Metadata describing a validated Training Dataset."""

    path: Path
    is_archive: bool
    samples: tuple[DatasetSample, ...]
    sample_count: int
    trigger_phrase: str


def _is_hidden(p: Path) -> bool:
    """Check if any path component starts with a dot (hidden file / directory)."""
    return any(part.startswith(".") for part in p.parts)


def _is_zip_archive(path: Path) -> bool:
    """Check if the path represents a readable ZIP archive."""
    if not path.is_file():
        return False
    if path.suffix.lower() == ".zip":
        return True
    return zipfile.is_zipfile(path)


def validate_dataset(
    dataset_path: str | Path,
    trigger_phrase: str = DEFAULT_TRIGGER_PHRASE,
) -> DatasetInfo:
    """Validate a Training Dataset directory or ZIP archive.

    Verifies:
    1. The dataset exists and is accessible.
    2. Contains at least one valid .png + .txt sample pair (non-empty).
    3. Every .png image has a companion .txt caption file.
    4. Every .txt caption begins with the specified trigger_phrase at the head.

    Args:
        dataset_path: Path to dataset folder or .zip file.
        trigger_phrase: Expected Trigger Phrase prefix (defaults to 'dungeonforge battlemap').

    Returns:
        DatasetInfo detailing the validated samples.

    Raises:
        DatasetValidationError: When validation fails (naming the defect and offending sample).
    """
    path = Path(dataset_path).resolve()

    if not path.exists():
        raise DatasetValidationError(
            code="dataset_not_found",
            message=f"Dataset path does not exist: {path}",
            sample=None,
        )

    if path.is_file():
        return _validate_zip_dataset(path, trigger_phrase)
    elif path.is_dir():
        return _validate_dir_dataset(path, trigger_phrase)
    else:
        raise DatasetValidationError(
            code="invalid_dataset_type",
            message=f"Dataset path is neither a regular file nor a directory: {path}",
            sample=None,
        )


def _validate_zip_dataset(
    zip_path: Path,
    trigger_phrase: str,
) -> DatasetInfo:
    """Validate contents of a ZIP dataset archive."""
    if not _is_zip_archive(zip_path):
        raise DatasetValidationError(
            code="invalid_archive",
            message=f"File '{zip_path.name}' is not a valid ZIP archive",
            sample=None,
        )

    try:
        with zipfile.ZipFile(zip_path, "r") as archive:
            entries = archive.namelist()
            # Filter out directories and OS noise (__MACOSX, .DS_Store, dot-files)
            valid_entries = [
                e
                for e in entries
                if not e.endswith("/")
                and not any(part.startswith(".") for part in Path(e).parts)
                and "__MACOSX" not in e
            ]

            # Build entry lookup mapping lower-cased path to actual entry
            entry_map = {e.lower(): e for e in valid_entries}

            # Find all .png entries
            png_entries = sorted(
                [e for e in valid_entries if e.lower().endswith(".png")],
                key=lambda x: x.lower(),
            )

            if not png_entries:
                raise DatasetValidationError(
                    code="empty_dataset",
                    message=f"Dataset archive '{zip_path.name}' contains zero .png sample images",
                    sample=None,
                )

            samples: list[DatasetSample] = []

            for png in png_entries:
                png_path_obj = Path(png)
                # Companion txt should share stem and directory
                txt_expected_path = str(png_path_obj.with_suffix(".txt")).lower()

                if txt_expected_path not in entry_map:
                    raise DatasetValidationError(
                        code="missing_caption",
                        message=(
                            f"Sample '{png_path_obj.name}' is missing companion caption file "
                            f"'{png_path_obj.with_suffix('.txt').name}'"
                        ),
                        sample=png_path_obj.name,
                    )

                actual_txt_entry = entry_map[txt_expected_path]
                try:
                    caption_bytes = archive.read(actual_txt_entry)
                    caption_text = caption_bytes.decode("utf-8-sig").strip()
                except Exception as exc:
                    raise DatasetValidationError(
                        code="corrupt_caption",
                        message=f"Could not read caption file '{Path(actual_txt_entry).name}': {exc}",
                        sample=Path(actual_txt_entry).name,
                    ) from exc

                txt_name = Path(actual_txt_entry).name
                if not caption_text:
                    raise DatasetValidationError(
                        code="missing_trigger_phrase",
                        message=(
                            f"Sample caption in '{txt_name}' is empty; "
                            f"must begin with trigger phrase '{trigger_phrase}'"
                        ),
                        sample=txt_name,
                    )

                if not caption_text.startswith(trigger_phrase):
                    preview = caption_text[:60].replace("\n", " ")
                    raise DatasetValidationError(
                        code="missing_trigger_phrase",
                        message=(
                            f"Caption in '{txt_name}' does not start with trigger phrase "
                            f"'{trigger_phrase}' (starts with: '{preview} ...')"
                        ),
                        sample=txt_name,
                    )

                samples.append(
                    DatasetSample(
                        image_path=png,
                        caption_path=actual_txt_entry,
                        caption_text=caption_text,
                    )
                )

            if not samples:
                raise DatasetValidationError(
                    code="empty_dataset",
                    message=f"Dataset archive '{zip_path.name}' contains zero valid .png+.txt sample pairs",
                    sample=None,
                )

            return DatasetInfo(
                path=zip_path,
                is_archive=True,
                samples=tuple(samples),
                sample_count=len(samples),
                trigger_phrase=trigger_phrase,
            )

    except zipfile.BadZipFile as exc:
        raise DatasetValidationError(
            code="invalid_archive",
            message=f"File '{zip_path.name}' is corrupt or not a valid ZIP archive: {exc}",
            sample=None,
        ) from exc


def _validate_dir_dataset(
    dir_path: Path,
    trigger_phrase: str,
) -> DatasetInfo:
    """Validate contents of a directory dataset."""
    # Recursively find all .png files, ignoring hidden files/folders
    all_files = [p for p in dir_path.rglob("*") if p.is_file() and not _is_hidden(p)]
    png_files = sorted(
        [p for p in all_files if p.suffix.lower() == ".png"],
        key=lambda x: str(x).lower(),
    )

    if not png_files:
        raise DatasetValidationError(
            code="empty_dataset",
            message=f"Dataset directory '{dir_path}' contains zero .png sample images",
            sample=None,
        )

    samples: list[DatasetSample] = []

    for png in png_files:
        companion_txt = png.with_suffix(".txt")
        if not companion_txt.is_file():
            raise DatasetValidationError(
                code="missing_caption",
                message=(
                    f"Sample '{png.name}' is missing companion caption file "
                    f"'{companion_txt.name}'"
                ),
                sample=png.name,
            )

        try:
            caption_text = companion_txt.read_text(encoding="utf-8-sig").strip()
        except Exception as exc:
            raise DatasetValidationError(
                code="corrupt_caption",
                message=f"Could not read caption file '{companion_txt.name}': {exc}",
                sample=companion_txt.name,
            ) from exc

        if not caption_text:
            raise DatasetValidationError(
                code="missing_trigger_phrase",
                message=(
                    f"Sample caption in '{companion_txt.name}' is empty; "
                    f"must begin with trigger phrase '{trigger_phrase}'"
                ),
                sample=companion_txt.name,
            )

        if not caption_text.startswith(trigger_phrase):
            preview = caption_text[:60].replace("\n", " ")
            raise DatasetValidationError(
                code="missing_trigger_phrase",
                message=(
                    f"Caption in '{companion_txt.name}' does not start with trigger phrase "
                    f"'{trigger_phrase}' (starts with: '{preview} ...')"
                ),
                sample=companion_txt.name,
            )

        samples.append(
            DatasetSample(
                image_path=str(png),
                caption_path=str(companion_txt),
                caption_text=caption_text,
            )
        )

    if not samples:
        raise DatasetValidationError(
            code="empty_dataset",
            message=f"Dataset directory '{dir_path}' contains zero valid .png+.txt sample pairs",
            sample=None,
        )

    return DatasetInfo(
        path=dir_path,
        is_archive=False,
        samples=tuple(samples),
        sample_count=len(samples),
        trigger_phrase=trigger_phrase,
    )


def extract_dataset_if_archive(dataset_path: str | Path, target_dir: str | Path) -> Path:
    """Extract a ZIP dataset into target_dir if it is an archive, else return the directory path.

    Convenience helper for training runners (e.g. Modal runner) preparing the local filesystem.
    """
    path = Path(dataset_path).resolve()
    target = Path(target_dir).resolve()

    if path.is_file() and _is_zip_archive(path):
        target.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(path, "r") as archive:
            archive.extractall(target)
        return target
    return path
