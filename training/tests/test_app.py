"""Unit tests for FastAPI endpoints: POST /run, GET /run/{id}, POST /run/{id}/cancel."""

import os
from unittest.mock import MagicMock
from fastapi.testclient import TestClient
import pytest

from dungeonforge_training.app import (
    fastapi_app,
    get_active_store,
    set_active_runner_manager,
    set_active_store,
)
from dungeonforge_training.runner import InMemoryRunStore, LocalRunnerManager, RunState


@pytest.fixture(autouse=True)
def setup_test_app(monkeypatch):
    store = InMemoryRunStore()
    set_active_store(store)
    mock_manager = MagicMock(spec=LocalRunnerManager)
    set_active_runner_manager(mock_manager)
    yield store, mock_manager


@pytest.fixture
def client():
    return TestClient(fastapi_app)


def test_auth_enforced_when_token_set(client, monkeypatch):
    monkeypatch.setenv("CURATOR_MODAL_TOKEN", "secret-test-token")

    # Missing header -> 401
    resp = client.get("/run/nonexistent")
    assert resp.status_code == 401
    assert "Invalid or missing" in resp.json()["detail"]

    # Wrong token -> 401
    resp = client.get("/run/nonexistent", headers={"Authorization": "Bearer wrong-token"})
    assert resp.status_code == 401
    assert "Invalid bearer token" in resp.json()["detail"]

    # Correct token, unknown id -> 404
    resp = client.get("/run/nonexistent", headers={"Authorization": "Bearer secret-test-token"})
    assert resp.status_code == 404


def test_auth_bypassed_when_no_token_set(client, monkeypatch):
    monkeypatch.delenv("CURATOR_MODAL_TOKEN", raising=False)
    resp = client.get("/run/nonexistent")
    assert resp.status_code == 404


def test_post_run_validation(client):
    # Missing dataset_url -> 422
    resp = client.post("/run", json={"run_policy": "calibration"})
    assert resp.status_code == 422

    # Invalid recipe (e.g. negative steps) -> 400
    resp = client.post("/run", json={"dataset_url": "https://example.com/ds.zip", "steps": -100})
    assert resp.status_code == 400


def test_post_run_success_top_level_recipe(client, setup_test_app):
    store, mock_manager = setup_test_app
    payload = {
        "dataset_url": "https://example.com/dataset.zip",
        "run_policy": "calibration",
        "steps": 1500,
    }
    resp = client.post("/run", json=payload)
    assert resp.status_code == 200
    data = resp.json()

    assert "run_id" in data
    assert data["state"] == "queued"
    assert data["step"] == 0
    assert data["total_steps"] == 1500
    assert data["checkpoints"] == []
    assert data["error"] is None

    # Verify run recorded in store and manager spawned worker
    run_record = store.get(data["run_id"])
    assert run_record is not None
    assert mock_manager.spawn.called


def test_post_run_success_nested_recipe(client, setup_test_app):
    store, mock_manager = setup_test_app
    payload = {
        "dataset_url": "https://example.com/dataset.zip",
        "run_id": "custom-run-001",
        "recipe": {
            "run_policy": "calibration",
            "steps": 1500,
        },
    }
    resp = client.post("/run", json=payload)
    assert resp.status_code == 200
    data = resp.json()

    assert data["run_id"] == "custom-run-001"
    assert data["state"] == "queued"
    assert data["total_steps"] == 1500


def test_get_run_status(client, setup_test_app):
    store, _ = setup_test_app
    # 404 for unknown run
    resp = client.get("/run/unknown-999")
    assert resp.status_code == 404

    # Seed store with active run
    store.set(
        "run-abc",
        {
            "run_id": "run-abc",
            "state": RunState.RUNNING.value,
            "step": 250,
            "total_steps": 1500,
            "started_at": 1000.0,
            "elapsed_seconds": 120.0,
            "cost_usd": 0.065,
            "checkpoints": [
                {
                    "step": 250,
                    "artifact_url": "https://pub.example.com/artifacts/runs/run-abc/checkpoints/step-250/adapter.safetensors",
                    "artifact_sha256": "abc",
                    "artifact_size": 100,
                    "sample_grid_url": "https://pub.example.com/artifacts/runs/run-abc/checkpoints/step-250/samples.png",
                    "sample_grid_sha256": "def",
                    "sample_grid_size": 200,
                }
            ],
            "error": None,
        },
    )

    resp = client.get("/run/run-abc")
    assert resp.status_code == 200
    data = resp.json()
    assert data["run_id"] == "run-abc"
    assert data["state"] == "running"
    assert data["step"] == 250
    assert data["total_steps"] == 1500
    assert len(data["checkpoints"]) == 1
    assert data["cost_usd"] >= 0.0


def test_post_run_cancel(client, setup_test_app):
    store, mock_manager = setup_test_app

    # 404 for unknown run
    resp = client.post("/run/unknown-999/cancel")
    assert resp.status_code == 404

    # Cancel active run
    store.set(
        "run-to-cancel",
        {
            "run_id": "run-to-cancel",
            "state": RunState.RUNNING.value,
            "step": 250,
            "total_steps": 1500,
            "started_at": 100.0,
            "checkpoints": [],
            "error": None,
        },
    )

    resp = client.post("/run/run-to-cancel/cancel")
    assert resp.status_code == 200
    data = resp.json()
    assert data["state"] == "cancelled"
    assert mock_manager.cancel.called

    # Cancel on already terminal run returns current state
    resp = client.post("/run/run-to-cancel/cancel")
    assert resp.status_code == 200
    assert resp.json()["state"] == "cancelled"
