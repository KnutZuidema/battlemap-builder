"""Exceptions and validation errors for dungeonforge-training."""

from typing import Optional


class TrainingError(Exception):
    """Base class for all errors in dungeonforge-training."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message

    def __str__(self) -> str:
        return f"[{self.code}] {self.message}"


class DatasetValidationError(TrainingError):
    """Raised when a dataset fails structural or content validation.

    Examples of defect codes:
    - empty_dataset: zero valid .png+.txt pairs in directory or ZIP
    - missing_caption: a .png image lacks its companion .txt file
    - missing_trigger_phrase: a .txt caption does not start with the expected trigger phrase
    - dataset_not_found: dataset path does not exist
    - invalid_archive: dataset file is not a valid ZIP archive
    """

    def __init__(
        self,
        code: str,
        message: str,
        sample: Optional[str] = None,
    ) -> None:
        super().__init__(code, message)
        self.sample = sample

    def __str__(self) -> str:
        if self.sample:
            return f"[{self.code}] Sample '{self.sample}': {self.message}"
        return f"[{self.code}] {self.message}"


class RecipeValidationError(TrainingError):
    """Raised when a training recipe contains invalid or contradictory parameters."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(code, message)
