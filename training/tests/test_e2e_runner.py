"""Comprehensive end-to-end test suite for Modal training runner (Issue #206).

Conforms to ADR-0008 and ADR-0009:
- Validates 1,500-step calibration run with checkpoints at 250, 500, 750, 1000, 1250, 1500
- Tests live progress polling (state, step, total_steps, elapsed_seconds, cost_usd)
- Verifies every checkpoint .safetensors and sample grid .png is uploaded to R2/S3
  and downloadable over HTTP with matching SHA-256 and metadata
- Tests failing runs (dataset validation failure & ai-toolkit runtime failure)
  ensuring exact error reporting and zero partial/corrupt artifacts in R2
- Tests cancellation via POST /run/{id}/cancel
"""

from functools import partial
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time
import zipfile

from PIL import Image
from fastapi.testclient import TestClient
import httpx
from moto.server import ThreadedMotoServer
import pytest

from dungeonforge_training.app import (
    fastapi_app,
    set_active_runner_manager,
    set_active_store,
    set_active_uploader,
)
from dungeonforge_training.errors import TrainingError
from dungeonforge_training.runner import InMemoryRunStore, LocalRunnerManager, RunState
from dungeonforge_training.storage import R2ArtifactUploader


@pytest.fixture(scope="module")
def local_dataset_server(tmp_path_factory):
    """Spin up a local HTTP server serving valid and invalid dataset ZIP archives."""
    ds_dir = tmp_path_factory.mktemp("datasets")

    # 1. Valid dataset ZIP: Ground-layer 1024x1024 PNG + companion TXT with trigger phrase
    valid_zip_path = ds_dir / "valid_dataset.zip"
    with zipfile.ZipFile(valid_zip_path, "w") as zf:
        img = Image.new("RGB", (1024, 1024), color=(30, 80, 120))
        img_bytes_path = ds_dir / "dungeon_01_Ground.png"
        img.save(img_bytes_path, format="PNG")
        zf.write(img_bytes_path, arcname="dungeon_01_Ground.png")

        caption = "dungeonforge battlemap, stone dungeon corridor with torchlight and iron portcullis"
        zf.writestr("dungeon_01_Ground.txt", caption)

    # 2. Invalid dataset ZIP: caption missing trigger phrase
    invalid_zip_path = ds_dir / "invalid_dataset.zip"
    with zipfile.ZipFile(invalid_zip_path, "w") as zf:
        zf.write(img_bytes_path, arcname="dungeon_01_Ground.png")
        zf.writestr("dungeon_01_Ground.txt", "missing required trigger token, plain description")

    handler = partial(SimpleHTTPRequestHandler, directory=str(ds_dir))
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()

    host, port = server.server_address
    yield f"http://{host}:{port}"

    server.shutdown()
    server.server_close()


@pytest.fixture(scope="module")
def local_s3_storage():
    """Spin up a local S3/R2 mock HTTP server allowing unauthenticated public-read downloads."""
    moto_server = ThreadedMotoServer("127.0.0.1", 0)
    moto_server.start()
    host, port = moto_server.get_host_and_port()
    endpoint_url = f"http://{host}:{port}"
    bucket_name = "test-r2-bucket"
    public_url = f"http://{host}:{port}/{bucket_name}"

    import boto3

    s3 = boto3.client(
        "s3",
        endpoint_url=endpoint_url,
        aws_access_key_id="mock-key",
        aws_secret_access_key="mock-secret",
        region_name="us-east-1",
    )
    s3.create_bucket(Bucket=bucket_name)

    # Configure bucket policy for public GET access (simulating Cloudflare R2 public bucket)
    policy = {
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": f"arn:aws:s3:::{bucket_name}/*",
            }
        ],
    }
    s3.put_bucket_policy(Bucket=bucket_name, Policy=json.dumps(policy))

    yield {
        "s3_client": s3,
        "endpoint_url": endpoint_url,
        "public_url": public_url,
        "bucket_name": bucket_name,
    }

    moto_server.stop()


@pytest.fixture
def e2e_env(local_s3_storage, monkeypatch):
    """Configure FastAPI app environment with isolated store, runner, and uploader."""
    monkeypatch.setenv("CURATOR_MODAL_TOKEN", "e2e-secret-token")
    monkeypatch.setenv("R2_ENDPOINT", local_s3_storage["endpoint_url"])
    monkeypatch.setenv("R2_PUBLIC_URL", local_s3_storage["public_url"])
    monkeypatch.setenv("R2_BUCKET_NAME", local_s3_storage["bucket_name"])
    monkeypatch.setenv("R2_ACCESS_KEY_ID", "mock-key")
    monkeypatch.setenv("R2_SECRET_ACCESS_KEY", "mock-secret")

    store = InMemoryRunStore()
    set_active_store(store)

    uploader = R2ArtifactUploader(
        endpoint_url=local_s3_storage["endpoint_url"],
        bucket_name=local_s3_storage["bucket_name"],
        access_key_id="mock-key",
        secret_access_key="mock-secret",
        public_url=local_s3_storage["public_url"],
        s3_client=local_s3_storage["s3_client"],
    )
    set_active_uploader(uploader)

    client = TestClient(fastapi_app)
    return {
        "client": client,
        "store": store,
        "uploader": uploader,
        "s3_client": local_s3_storage["s3_client"],
        "bucket_name": local_s3_storage["bucket_name"],
        "auth_headers": {"Authorization": "Bearer e2e-secret-token"},
    }


def test_e2e_1500_step_calibration_run(e2e_env, local_dataset_server):
    """E2E Test: 1,500-step calibration run with checkpoints at 250, 500, 750, 1000, 1250, 1500."""
    client = e2e_env["client"]
    store = e2e_env["store"]
    uploader = e2e_env["uploader"]
    auth_headers = e2e_env["auth_headers"]

    # Define training executor simulating 1500-step calibration run
    def simulated_calibration_trainer(
        run_id, config_path, job_dir, samples_dir, checkpoint_steps, checkpoint_callback, cancel_event
    ):
        for step in checkpoint_steps:
            if cancel_event and cancel_event.is_set():
                return

            # 1. Create weights file (.safetensors)
            weights_file = job_dir / f"{run_id}_{step}.safetensors"
            weights_file.write_bytes(f"weights_step_{step}_{'x'*1000}".encode())

            # For final step, also write root model weights
            if step == 1500:
                final_model = job_dir / f"{run_id}.safetensors"
                final_model.write_bytes(f"weights_final_{'y'*1000}".encode())

            # 2. Create 4 sample preview images (1024x1024)
            for idx in range(4):
                sample_path = samples_dir / f"{run_id}_prompt_{idx}_step_{step}.png"
                img = Image.new("RGB", (1024, 1024), color=(idx * 40, (step // 10) % 256, 120))
                img.save(sample_path, format="PNG")

            # 3. Trigger checkpoint callback
            checkpoint_callback(step)
            time.sleep(0.02)

    manager = LocalRunnerManager(store=store, training_executor=simulated_calibration_trainer)
    set_active_runner_manager(manager)

    # 1. Submit run via POST /run
    dataset_url = f"{local_dataset_server}/valid_dataset.zip"
    post_payload = {
        "dataset_url": dataset_url,
        "run_policy": "calibration",
        "steps": 1500,
        "save_every": 250,
        "sample_every": 250,
    }

    submit_resp = client.post("/run", json=post_payload, headers=auth_headers)
    assert submit_resp.status_code == 200, submit_resp.text
    submit_data = submit_resp.json()

    run_id = submit_data["run_id"]
    assert run_id
    assert submit_data["state"] in (RunState.QUEUED.value, RunState.RUNNING.value)
    assert submit_data["total_steps"] == 1500

    # 2. Poll live progress via GET /run/{run_id}
    poll_resp = client.get(f"/run/{run_id}", headers=auth_headers)
    assert poll_resp.status_code == 200
    poll_data = poll_resp.json()
    assert poll_data["run_id"] == run_id
    assert poll_data["state"] in (RunState.QUEUED.value, RunState.RUNNING.value, RunState.SUCCEEDED.value)
    assert poll_data["total_steps"] == 1500
    assert poll_data["elapsed_seconds"] >= 0.0
    assert poll_data["cost_usd"] >= 0.0
    assert poll_data["error"] is None

    # 3. Wait for run to succeed
    deadline = time.time() + 15.0
    final_data = None
    while time.time() < deadline:
        res = client.get(f"/run/{run_id}", headers=auth_headers)
        assert res.status_code == 200
        cur = res.json()
        if cur["state"] == RunState.SUCCEEDED.value:
            final_data = cur
            break
        elif cur["state"] in (RunState.FAILED.value, RunState.CANCELLED.value):
            pytest.fail(f"Run entered unexpected terminal state: {cur}")
        time.sleep(0.1)

    assert final_data is not None, "Timed out waiting for run to complete"
    assert final_data["state"] == RunState.SUCCEEDED.value
    assert final_data["step"] == 1500
    assert final_data["total_steps"] == 1500
    assert final_data["elapsed_seconds"] >= 0.0
    assert final_data["cost_usd"] >= 0.0
    assert final_data["error"] is None

    # Verify all 6 checkpoints: 250, 500, 750, 1000, 1250, 1500
    checkpoints = final_data["checkpoints"]
    assert len(checkpoints) == 6
    expected_steps = [250, 500, 750, 1000, 1250, 1500]
    actual_steps = [cp["step"] for cp in checkpoints]
    assert actual_steps == expected_steps

    # 4. Verify each checkpoint is downloadable over HTTP with matching SHA-256
    with httpx.Client(timeout=10.0) as http_client:
        for cp in checkpoints:
            step = cp["step"]

            # Download adapter weights
            adapter_url = cp["artifact_url"]
            assert f"artifacts/runs/{run_id}/checkpoints/step-{step}/adapter.safetensors" in adapter_url
            resp_weights = http_client.get(adapter_url)
            assert resp_weights.status_code == 200
            weights_content = resp_weights.content
            assert len(weights_content) == cp["artifact_size"]
            computed_sha = hashlib.sha256(weights_content).hexdigest()
            assert computed_sha == cp["artifact_sha256"]
            assert resp_weights.headers.get("x-amz-meta-sha256") == cp["artifact_sha256"]

            # Download sample grid
            samples_url = cp["sample_grid_url"]
            assert f"artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png" in samples_url
            resp_sample = http_client.get(samples_url)
            assert resp_sample.status_code == 200
            sample_content = resp_sample.content
            assert len(sample_content) == cp["sample_grid_size"]
            computed_sample_sha = hashlib.sha256(sample_content).hexdigest()
            assert computed_sample_sha == cp["sample_grid_sha256"]
            assert resp_sample.headers.get("x-amz-meta-sha256") == cp["sample_grid_sha256"]


def test_e2e_failing_run_dataset_validation(e2e_env, local_dataset_server):
    """E2E Test: Run fails fast during dataset validation, leaves zero partial artifacts in R2."""
    client = e2e_env["client"]
    store = e2e_env["store"]
    s3_client = e2e_env["s3_client"]
    bucket_name = e2e_env["bucket_name"]
    auth_headers = e2e_env["auth_headers"]

    manager = LocalRunnerManager(store=store)
    set_active_runner_manager(manager)

    invalid_dataset_url = f"{local_dataset_server}/invalid_dataset.zip"
    post_payload = {
        "dataset_url": invalid_dataset_url,
        "run_policy": "calibration",
        "steps": 1500,
    }

    submit_resp = client.post("/run", json=post_payload, headers=auth_headers)
    assert submit_resp.status_code == 200
    run_id = submit_resp.json()["run_id"]

    # Poll until failure
    deadline = time.time() + 10.0
    failed_data = None
    while time.time() < deadline:
        res = client.get(f"/run/{run_id}", headers=auth_headers)
        data = res.json()
        if data["state"] == RunState.FAILED.value:
            failed_data = data
            break
        time.sleep(0.1)

    assert failed_data is not None
    assert failed_data["state"] == RunState.FAILED.value
    assert "Dataset validation failed" in failed_data["error"]
    assert "missing_trigger_phrase" in failed_data["error"]
    assert failed_data["checkpoints"] == []

    # Verify zero artifacts in R2 bucket
    objs = s3_client.list_objects_v2(Bucket=bucket_name, Prefix=f"artifacts/runs/{run_id}/")
    assert objs.get("KeyCount", 0) == 0


def test_e2e_failing_run_aitoolkit_error(e2e_env, local_dataset_server):
    """E2E Test: Run fails during ai-toolkit training, surfaces error and leaves zero partial artifacts."""
    client = e2e_env["client"]
    store = e2e_env["store"]
    s3_client = e2e_env["s3_client"]
    bucket_name = e2e_env["bucket_name"]
    auth_headers = e2e_env["auth_headers"]

    def failing_trainer(
        run_id, config_path, job_dir, samples_dir, checkpoint_steps, checkpoint_callback, cancel_event
    ):
        raise TrainingError("aitoolkit_failed", "ai-toolkit failed (code 1): CUDA out of memory in diffusion_trainer")

    manager = LocalRunnerManager(store=store, training_executor=failing_trainer)
    set_active_runner_manager(manager)

    valid_dataset_url = f"{local_dataset_server}/valid_dataset.zip"
    post_payload = {
        "dataset_url": valid_dataset_url,
        "run_policy": "calibration",
        "steps": 1500,
    }

    submit_resp = client.post("/run", json=post_payload, headers=auth_headers)
    assert submit_resp.status_code == 200
    run_id = submit_resp.json()["run_id"]

    # Poll until failure
    deadline = time.time() + 10.0
    failed_data = None
    while time.time() < deadline:
        res = client.get(f"/run/{run_id}", headers=auth_headers)
        data = res.json()
        if data["state"] == RunState.FAILED.value:
            failed_data = data
            break
        time.sleep(0.1)

    assert failed_data is not None
    assert failed_data["state"] == RunState.FAILED.value
    assert "CUDA out of memory" in failed_data["error"]
    assert failed_data["checkpoints"] == []

    # Verify zero artifacts in R2 bucket
    objs = s3_client.list_objects_v2(Bucket=bucket_name, Prefix=f"artifacts/runs/{run_id}/")
    assert objs.get("KeyCount", 0) == 0


def test_e2e_cancel_active_run(e2e_env, local_dataset_server):
    """E2E Test: Cancelling an active running worker via POST /run/{id}/cancel."""
    client = e2e_env["client"]
    store = e2e_env["store"]
    auth_headers = e2e_env["auth_headers"]

    worker_started_event = threading.Event()

    def cancellable_trainer(
        run_id, config_path, job_dir, samples_dir, checkpoint_steps, checkpoint_callback, cancel_event
    ):
        worker_started_event.set()
        # Wait until cancelled or timeout
        if cancel_event:
            cancel_event.wait(timeout=5.0)

    manager = LocalRunnerManager(store=store, training_executor=cancellable_trainer)
    set_active_runner_manager(manager)

    valid_dataset_url = f"{local_dataset_server}/valid_dataset.zip"
    post_payload = {
        "dataset_url": valid_dataset_url,
        "run_policy": "calibration",
        "steps": 1500,
    }

    submit_resp = client.post("/run", json=post_payload, headers=auth_headers)
    assert submit_resp.status_code == 200
    run_id = submit_resp.json()["run_id"]

    # Wait until trainer actually starts
    worker_started_event.wait(timeout=5.0)

    # Cancel via POST /run/{id}/cancel
    cancel_resp = client.post(f"/run/{run_id}/cancel", headers=auth_headers)
    assert cancel_resp.status_code == 200
    cancel_data = cancel_resp.json()
    assert cancel_data["state"] == RunState.CANCELLED.value

    # Poll status to confirm state remains cancelled
    poll_resp = client.get(f"/run/{run_id}", headers=auth_headers)
    assert poll_resp.status_code == 200
    assert poll_resp.json()["state"] == RunState.CANCELLED.value
