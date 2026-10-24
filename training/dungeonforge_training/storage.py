"""Cloudflare R2 / S3 artifact storage and uploader for Dungeon Forge training runs.

Conforms to ADR-0008 (FLUX 3 Image Klein LoRA training stack) and
ADR-0009 (Training Run lifecycle, Dataset Snapshots, and LoRA Adapter publication).
All artifacts are uploaded under the public-read 'artifacts/' prefix with
x-amz-meta-sha256 metadata.
"""

from dataclasses import asdict, dataclass
import hashlib
import os
from pathlib import Path
from typing import Any, Mapping, Optional

import boto3
from botocore.config import Config


@dataclass(frozen=True)
class CheckpointArtifact:
    """Descriptor for an atomically uploaded training checkpoint in R2."""

    step: int
    artifact_url: str
    artifact_sha256: str
    artifact_size: int
    sample_grid_url: str
    sample_grid_sha256: str
    sample_grid_size: int

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary representation matching ADR-0009 run polling schema."""
        return asdict(self)


def compute_sha256_and_size(path: Path | str) -> tuple[str, int]:
    """Compute hex SHA-256 digest and total byte size for a local file."""
    p = Path(path)
    if not p.is_file():
        raise FileNotFoundError(f"File not found: {p}")
    hasher = hashlib.sha256()
    size = 0
    with p.open("rb") as f:
        while chunk := f.read(65536):
            hasher.update(chunk)
            size += len(chunk)
    return hasher.hexdigest(), size


class R2ArtifactUploader:
    """Manages atomic artifact publication to Cloudflare R2 / S3 storage."""

    def __init__(
        self,
        endpoint_url: Optional[str] = None,
        bucket_name: Optional[str] = None,
        access_key_id: Optional[str] = None,
        secret_access_key: Optional[str] = None,
        public_url: Optional[str] = None,
        region_name: str = "auto",
        s3_client: Optional[Any] = None,
    ) -> None:
        self.endpoint_url = (
            endpoint_url
            or os.environ.get("R2_ENDPOINT")
            or os.environ.get("AWS_ENDPOINT_URL_S3")
        )
        self.bucket_name = bucket_name or os.environ.get("R2_BUCKET_NAME", "dungeonforge-artifacts")
        self.access_key_id = (
            access_key_id
            or os.environ.get("R2_ACCESS_KEY_ID")
            or os.environ.get("AWS_ACCESS_KEY_ID")
        )
        self.secret_access_key = (
            secret_access_key
            or os.environ.get("R2_SECRET_ACCESS_KEY")
            or os.environ.get("AWS_SECRET_ACCESS_KEY")
        )
        raw_public_url = (
            public_url
            or os.environ.get("R2_PUBLIC_URL")
            or (self.endpoint_url.rstrip("/") + "/" + self.bucket_name if self.endpoint_url else "")
        )
        self.public_url = raw_public_url.rstrip("/") if raw_public_url else ""
        self.region_name = region_name or "auto"

        if s3_client is not None:
            self._client = s3_client
        else:
            self._client = boto3.client(
                "s3",
                endpoint_url=self.endpoint_url,
                aws_access_key_id=self.access_key_id,
                aws_secret_access_key=self.secret_access_key,
                region_name=self.region_name,
                config=Config(signature_version="s3v4", retries={"max_attempts": 3, "mode": "standard"}),
            )

    @property
    def client(self) -> Any:
        return self._client

    def upload_file(
        self,
        local_path: Path | str,
        object_key: str,
        content_type: str,
        sha256_hex: str,
    ) -> None:
        """Upload a single file with x-amz-meta-sha256 metadata."""
        path = Path(local_path)
        with path.open("rb") as f:
            self._client.put_object(
                Bucket=self.bucket_name,
                Key=object_key,
                Body=f,
                ContentType=content_type,
                Metadata={"sha256": sha256_hex},
            )

    def delete_file(self, object_key: str) -> None:
        """Delete an object from the bucket."""
        try:
            self._client.delete_object(Bucket=self.bucket_name, Key=object_key)
        except Exception:
            pass

    def upload_checkpoint(
        self,
        run_id: str,
        step: int,
        weights_path: Path | str,
        sample_grid_path: Path | str,
    ) -> CheckpointArtifact:
        """Atomically upload weights and sample grid for a checkpoint step.

        If either upload fails, cleans up any partially uploaded object for the step
        so that no partial/corrupt artifact remains in R2.
        """
        weights_p = Path(weights_path)
        sample_p = Path(sample_grid_path)

        if not weights_p.is_file():
            raise FileNotFoundError(f"Checkpoint weights not found: {weights_p}")
        if not sample_p.is_file():
            raise FileNotFoundError(f"Sample grid image not found: {sample_p}")

        adapter_sha, adapter_size = compute_sha256_and_size(weights_p)
        sample_sha, sample_size = compute_sha256_and_size(sample_p)

        adapter_key = f"artifacts/runs/{run_id}/checkpoints/step-{step}/adapter.safetensors"
        sample_key = f"artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png"

        uploaded_keys: list[str] = []
        try:
            self.upload_file(weights_p, adapter_key, "application/octet-stream", adapter_sha)
            uploaded_keys.append(adapter_key)

            self.upload_file(sample_p, sample_key, "image/png", sample_sha)
            uploaded_keys.append(sample_key)
        except Exception:
            for k in uploaded_keys:
                self.delete_file(k)
            raise

        adapter_url = f"{self.public_url}/{adapter_key}"
        sample_grid_url = f"{self.public_url}/{sample_key}"

        return CheckpointArtifact(
            step=step,
            artifact_url=adapter_url,
            artifact_sha256=adapter_sha,
            artifact_size=adapter_size,
            sample_grid_url=sample_grid_url,
            sample_grid_sha256=sample_sha,
            sample_grid_size=sample_size,
        )

    def delete_step_artifacts(self, run_id: str, step: int) -> None:
        """Clean up artifacts for a specific checkpoint step."""
        adapter_key = f"artifacts/runs/{run_id}/checkpoints/step-{step}/adapter.safetensors"
        sample_key = f"artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png"
        self.delete_file(adapter_key)
        self.delete_file(sample_key)

    def delete_run_artifacts(self, run_id: str) -> None:
        """Clean up all artifacts under artifacts/runs/{run_id}/."""
        prefix = f"artifacts/runs/{run_id}/"
        try:
            paginator = self._client.get_paginator("list_objects_v2")
            for page in paginator.paginate(Bucket=self.bucket_name, Prefix=prefix):
                contents = page.get("Contents", [])
                if contents:
                    delete_keys = [{"Key": obj["Key"]} for obj in contents]
                    self._client.delete_objects(
                        Bucket=self.bucket_name,
                        Delete={"Objects": delete_keys},
                    )
        except Exception:
            pass
