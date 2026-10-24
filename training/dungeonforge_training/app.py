"""FastAPI application and Modal deployment definition for Dungeon Forge training runner.

Conforms to ADR-0008 (FLUX 3 Image Klein LoRA training stack) and
ADR-0009 (Training Run lifecycle, Dataset Snapshots, and LoRA Adapter publication).
Exposes the three-endpoint contract behind CURATOR_MODAL_TOKEN:
  - POST /run: submit run and spawn worker asynchronously
  - GET /run/{id}: poll live progress, cost, step, and published checkpoints
  - POST /run/{id}/cancel: cancel active non-terminal run
"""

import os
from typing import Any, Optional
import uuid

from fastapi import Depends, FastAPI, Header, HTTPException, Request, status
from fastapi.responses import JSONResponse
import modal

from dungeonforge_training.errors import RecipeValidationError
from dungeonforge_training.recipe import TrainingRecipe
from dungeonforge_training.runner import (
    InMemoryRunStore,
    LocalRunnerManager,
    RunState,
    RunStore,
    calculate_cost_usd,
    execute_training_run,
    format_run_response,
    get_default_run_store,
)
from dungeonforge_training.storage import R2ArtifactUploader

# ─────────────────────────────────────────────────────────────────────────────
# Authentication Dependency
# ─────────────────────────────────────────────────────────────────────────────


def verify_bearer_token(authorization: Optional[str] = Header(None)) -> None:
    """Validate bearer token against CURATOR_MODAL_TOKEN if configured."""
    expected_token = os.environ.get("CURATOR_MODAL_TOKEN")
    if not expected_token:
        # If no token is configured in the environment, bypass auth (e.g. open test setup)
        return

    if not authorization or not authorization.startswith("Bearer "):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid or missing bearer token",
            headers={"WWW-Authenticate": "Bearer"},
        )

    token = authorization.removeprefix("Bearer ").strip()
    if token != expected_token:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid bearer token",
            headers={"WWW-Authenticate": "Bearer"},
        )


# ─────────────────────────────────────────────────────────────────────────────
# FastAPI App Definition & State Management
# ─────────────────────────────────────────────────────────────────────────────

fastapi_app = FastAPI(
    title="Dungeon Forge Training Runner",
    description="Modal L40S training runner for FLUX 3 Image Klein LoRA fine-tuning",
    version="0.1.0",
)

_global_store: Optional[RunStore] = None
_global_runner_manager: Optional[LocalRunnerManager] = None
_global_uploader: Optional[R2ArtifactUploader] = None


def get_active_store() -> RunStore:
    global _global_store
    if _global_store is None:
        _global_store = get_default_run_store()
    return _global_store


def set_active_store(store: RunStore) -> None:
    global _global_store
    _global_store = store


def get_active_runner_manager() -> LocalRunnerManager:
    global _global_runner_manager
    if _global_runner_manager is None:
        _global_runner_manager = LocalRunnerManager(store=get_active_store())
    return _global_runner_manager


def set_active_runner_manager(manager: LocalRunnerManager) -> None:
    global _global_runner_manager
    _global_runner_manager = manager


def get_active_uploader() -> R2ArtifactUploader:
    global _global_uploader
    if _global_uploader is None:
        _global_uploader = R2ArtifactUploader()
    return _global_uploader


def set_active_uploader(uploader: R2ArtifactUploader) -> None:
    global _global_uploader
    _global_uploader = uploader


def _is_modal_worker_available() -> bool:
    """Check if we are running in an active Modal environment capable of modal spawn."""
    try:
        return not modal.is_local()
    except Exception:
        return False


# ─────────────────────────────────────────────────────────────────────────────
# HTTP Endpoints
# ─────────────────────────────────────────────────────────────────────────────


@fastapi_app.post("/run", dependencies=[Depends(verify_bearer_token)])
async def submit_run(request: Request) -> JSONResponse:
    """Submit a training run, initialize record, and spawn worker asynchronously."""
    try:
        body = await request.json()
    except Exception:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Request body must be valid JSON",
        )

    if not isinstance(body, dict):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Request payload must be a JSON object",
        )

    dataset_url = body.get("dataset_url")
    if not dataset_url or not isinstance(dataset_url, str):
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            detail="Missing or invalid required field 'dataset_url'",
        )

    raw_run_id = body.get("run_id")
    run_id = str(raw_run_id).strip() if raw_run_id else f"run-{uuid.uuid4().hex[:12]}"

    # Extract recipe (nested under 'recipe' or flattened at top-level)
    recipe_payload = body.get("recipe")
    if isinstance(recipe_payload, dict):
        recipe_dict = recipe_payload
    else:
        recipe_dict = body

    try:
        recipe = TrainingRecipe.from_dict(recipe_dict)
    except RecipeValidationError as exc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Invalid Training Recipe: {exc}",
        )

    store = get_active_store()
    init_data = {
        "run_id": run_id,
        "state": RunState.QUEUED.value,
        "step": 0,
        "total_steps": recipe.steps,
        "started_at": None,
        "finished_at": None,
        "elapsed_seconds": 0.0,
        "cost_usd": 0.0,
        "checkpoints": [],
        "error": None,
        "dataset_url": dataset_url,
        "recipe": recipe.to_dict(),
        "call_id": None,
    }
    store.set(run_id, init_data)

    # Spawn worker asynchronously
    if _is_modal_worker_available():
        call = train_worker.spawn(run_id, dataset_url, recipe.to_dict())
        store.update(run_id, call_id=call.object_id)
    else:
        manager = get_active_runner_manager()
        uploader = get_active_uploader()
        manager.spawn(run_id, dataset_url, recipe, uploader=uploader)

    response_data = format_run_response(init_data)
    return JSONResponse(status_code=status.HTTP_200_OK, content=response_data)


@fastapi_app.get("/run/{run_id}", dependencies=[Depends(verify_bearer_token)])
async def get_run_status(run_id: str) -> JSONResponse:
    """Poll run status, step progress, cost, and published checkpoints."""
    store = get_active_store()
    data = store.get(run_id)
    if data is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Run '{run_id}' not found",
        )

    return JSONResponse(status_code=status.HTTP_200_OK, content=format_run_response(data))


@fastapi_app.post("/run/{run_id}/cancel", dependencies=[Depends(verify_bearer_token)])
async def cancel_run(run_id: str) -> JSONResponse:
    """Cancel a non-terminal training run and stop the worker."""
    store = get_active_store()
    data = store.get(run_id)
    if data is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Run '{run_id}' not found",
        )

    current_state = data.get("state")
    if current_state in (RunState.SUCCEEDED.value, RunState.FAILED.value, RunState.CANCELLED.value):
        # Already terminal, return current record
        return JSONResponse(status_code=status.HTTP_200_OK, content=format_run_response(data))

    # Stop local worker if running locally
    manager = get_active_runner_manager()
    manager.cancel(run_id)

    # Stop Modal worker call if call_id recorded
    call_id = data.get("call_id")
    if call_id:
        try:
            modal.FunctionCall.from_id(call_id).cancel()
        except Exception:
            pass

    import time

    now = time.time()
    started_at = data.get("started_at") or now
    elapsed = now - started_at
    cost = calculate_cost_usd(elapsed)

    data["state"] = RunState.CANCELLED.value
    data["finished_at"] = now
    data["elapsed_seconds"] = round(elapsed, 2)
    data["cost_usd"] = cost
    store.set(run_id, data)

    return JSONResponse(status_code=status.HTTP_200_OK, content=format_run_response(data))


# ─────────────────────────────────────────────────────────────────────────────
# Modal App & GPU Worker Definition
# ─────────────────────────────────────────────────────────────────────────────

modal_app = modal.App("curator-training-runner")

training_image = (
    modal.Image.debian_slim(python_version="3.11")
    .apt_install("git")
    .pip_install(
        "torch>=2.4.0",
        "torchvision",
        "safetensors",
        "diffusers>=0.30.0",
        "transformers>=4.44.0",
        "accelerate>=0.33.0",
        "fastapi>=0.100.0",
        "uvicorn>=0.20.0",
        "boto3>=1.28.0",
        "pillow>=10.0.0",
        "pyyaml>=6.0",
        "httpx>=0.24.0",
        "pydantic>=2.0.0",
    )
    .run_commands(
        "git clone https://github.com/ostris/ai-toolkit.git /opt/ai-toolkit",
        "cd /opt/ai-toolkit && git submodule update --init --recursive && pip install -r requirements.txt",
    )
    .add_local_python_source("dungeonforge_training")
)

curator_secrets = modal.Secret.from_name("curator-secrets")


@modal_app.function(
    gpu="L40S",
    timeout=7200,
    image=training_image,
    secrets=[curator_secrets],
)
def train_worker(run_id: str, dataset_url: str, recipe_dict: dict[str, Any]) -> None:
    """Modal L40S worker executing the ai-toolkit fine-tuning run."""
    recipe = TrainingRecipe.from_dict(recipe_dict)
    store = get_default_run_store()
    uploader = R2ArtifactUploader()
    execute_training_run(
        run_id=run_id,
        dataset_url=dataset_url,
        recipe=recipe,
        store=store,
        uploader=uploader,
    )


@modal_app.function(
    image=training_image,
    secrets=[curator_secrets],
)
@modal.asgi_app()
def app() -> FastAPI:
    """Serve the FastAPI application as the Modal web endpoint."""
    return fastapi_app
