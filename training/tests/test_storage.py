"""Tests for Cloudflare R2 / S3 storage uploader and artifact publishing."""

import io
from pathlib import Path
import tempfile
import pytest
import boto3
from moto import mock_aws

from dungeonforge_training.storage import (
    CheckpointArtifact,
    R2ArtifactUploader,
    compute_sha256_and_size,
)


@pytest.fixture
def temp_files(tmp_path: Path):
    weights = tmp_path / "model.safetensors"
    weights.write_bytes(b"dummy_weights_content_12345")

    sample = tmp_path / "samples.png"
    sample.write_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDRdummy_png_bytes")

    return weights, sample


def test_compute_sha256_and_size(temp_files):
    weights, _ = temp_files
    sha, size = compute_sha256_and_size(weights)
    assert len(sha) == 64
    assert size == len(b"dummy_weights_content_12345")


@mock_aws
def test_r2_upload_checkpoint_success(temp_files):
    weights, sample = temp_files
    s3 = boto3.client("s3", region_name="us-east-1")
    s3.create_bucket(Bucket="test-bucket")

    uploader = R2ArtifactUploader(
        endpoint_url="https://r2.example.com",
        bucket_name="test-bucket",
        access_key_id="test",
        secret_access_key="test",
        public_url="https://pub.example.com",
        s3_client=s3,
    )

    artifact = uploader.upload_checkpoint(
        run_id="run-123",
        step=250,
        weights_path=weights,
        sample_grid_path=sample,
    )

    assert artifact.step == 250
    assert artifact.artifact_url == "https://pub.example.com/artifacts/runs/run-123/checkpoints/step-250/adapter.safetensors"
    assert artifact.sample_grid_url == "https://pub.example.com/artifacts/runs/run-123/checkpoints/step-250/samples.png"
    assert artifact.artifact_size == weights.stat().st_size
    assert artifact.sample_grid_size == sample.stat().st_size

    # Verify S3 objects exist with metadata
    obj_weights = s3.get_object(Bucket="test-bucket", Key="artifacts/runs/run-123/checkpoints/step-250/adapter.safetensors")
    assert obj_weights["Metadata"]["sha256"] == artifact.artifact_sha256
    assert obj_weights["ContentType"] == "application/octet-stream"

    obj_sample = s3.get_object(Bucket="test-bucket", Key="artifacts/runs/run-123/checkpoints/step-250/samples.png")
    assert obj_sample["Metadata"]["sha256"] == artifact.sample_grid_sha256
    assert obj_sample["ContentType"] == "image/png"


@mock_aws
def test_r2_upload_checkpoint_atomic_cleanup_on_failure(temp_files, monkeypatch):
    weights, sample = temp_files
    s3 = boto3.client("s3", region_name="us-east-1")
    s3.create_bucket(Bucket="test-bucket")

    uploader = R2ArtifactUploader(
        endpoint_url="https://r2.example.com",
        bucket_name="test-bucket",
        access_key_id="test",
        secret_access_key="test",
        public_url="https://pub.example.com",
        s3_client=s3,
    )

    # Monkeypatch second upload (samples.png) to fail
    original_upload = uploader.upload_file
    call_count = 0

    def failing_upload(local_path, object_key, content_type, sha256_hex):
        nonlocal call_count
        call_count += 1
        if "samples.png" in object_key:
            raise RuntimeError("Simulated network failure on sample upload")
        return original_upload(local_path, object_key, content_type, sha256_hex)

    monkeypatch.setattr(uploader, "upload_file", failing_upload)

    with pytest.raises(RuntimeError, match="Simulated network failure"):
        uploader.upload_checkpoint(
            run_id="run-fail-123",
            step=500,
            weights_path=weights,
            sample_grid_path=sample,
        )

    # Ensure adapter.safetensors was cleaned up so no partial artifact remains
    adapter_key = "artifacts/runs/run-fail-123/checkpoints/step-500/adapter.safetensors"
    with pytest.raises(boto3.client("s3").exceptions.NoSuchKey):
        s3.get_object(Bucket="test-bucket", Key=adapter_key)


@mock_aws
def test_delete_run_artifacts(temp_files):
    weights, sample = temp_files
    s3 = boto3.client("s3", region_name="us-east-1")
    s3.create_bucket(Bucket="test-bucket")

    uploader = R2ArtifactUploader(
        bucket_name="test-bucket",
        s3_client=s3,
    )

    uploader.upload_checkpoint("run-del", 250, weights, sample)
    uploader.upload_checkpoint("run-del", 500, weights, sample)

    # Verify 4 objects created
    objs = s3.list_objects_v2(Bucket="test-bucket", Prefix="artifacts/runs/run-del/")
    assert objs["KeyCount"] == 4

    uploader.delete_run_artifacts("run-del")

    objs_after = s3.list_objects_v2(Bucket="test-bucket", Prefix="artifacts/runs/run-del/")
    assert objs_after.get("KeyCount", 0) == 0
