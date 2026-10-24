"""Worker runner, state store, and training lifecycle management for Dungeon Forge.

Conforms to ADR-0008 and ADR-0009:
- Modal L40S rate: $1.95/hr
- Checkpoints every 250 steps
- Atomic upload of weights (.safetensors) and sample grid (samples.png)
- Surfaces exact ai-toolkit stderr / dataset errors on failure
- Clean state cancellation
"""

from dataclasses import asdict, dataclass, field
from enum import Enum
import logging
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import threading
import time
from typing import Any, Callable, Mapping, Optional
import urllib.request

import httpx

from dungeonforge_training.config import build_aitoolkit_config, emit_config
from dungeonforge_training.dataset import extract_dataset_if_archive, validate_dataset
from dungeonforge_training.errors import DatasetValidationError, TrainingError
from dungeonforge_training.grid import find_and_compose_step_grid
from dungeonforge_training.recipe import TrainingRecipe
from dungeonforge_training.storage import CheckpointArtifact, R2ArtifactUploader

logger = logging.getLogger(__name__)

L40S_HOURLY_RATE_USD = 1.95


class RunState(str, Enum):
    QUEUED = "queued"
    RUNNING = "running"
    SUCCEEDED = "succeeded"
    FAILED = "failed"
    CANCELLED = "cancelled"


def calculate_cost_usd(elapsed_seconds: float) -> float:
    """Calculate accumulated cost based on Modal L40S tier ($1.95/hr)."""
    return round((elapsed_seconds / 3600.0) * L40S_HOURLY_RATE_USD, 4)


class RunStore:
    """Interface for persisting and reading training run state."""

    def get(self, run_id: str) -> Optional[dict[str, Any]]:
        raise NotImplementedError

    def set(self, run_id: str, data: dict[str, Any]) -> None:
        raise NotImplementedError

    def update(self, run_id: str, **kwargs: Any) -> Optional[dict[str, Any]]:
        raise NotImplementedError


class InMemoryRunStore(RunStore):
    """Thread-safe in-memory store for local testing and execution."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._runs: dict[str, dict[str, Any]] = {}

    def get(self, run_id: str) -> Optional[dict[str, Any]]:
        with self._lock:
            data = self._runs.get(run_id)
            return dict(data) if data is not None else None

    def set(self, run_id: str, data: dict[str, Any]) -> None:
        with self._lock:
            self._runs[run_id] = dict(data)

    def update(self, run_id: str, **kwargs: Any) -> Optional[dict[str, Any]]:
        with self._lock:
            if run_id not in self._runs:
                return None
            run = dict(self._runs[run_id])
            run.update(kwargs)
            self._runs[run_id] = run
            return dict(run)

    def clear(self) -> None:
        with self._lock:
            self._runs.clear()


class ModalDictRunStore(RunStore):
    """Distributed key-value store wrapper around modal.Dict."""

    def __init__(self, dict_name: str = "curator-training-runs") -> None:
        import modal

        self._dict = modal.Dict.from_name(dict_name, create_if_missing=True)

    def get(self, run_id: str) -> Optional[dict[str, Any]]:
        try:
            return self._dict.get(run_id)
        except KeyError:
            return None

    def set(self, run_id: str, data: dict[str, Any]) -> None:
        self._dict[run_id] = data

    def update(self, run_id: str, **kwargs: Any) -> Optional[dict[str, Any]]:
        try:
            data = dict(self._dict[run_id])
            data.update(kwargs)
            self._dict[run_id] = data
            return data
        except KeyError:
            return None


def get_default_run_store() -> RunStore:
    """Determine the appropriate store based on whether we are in a Modal container."""
    try:
        import modal

        if not modal.is_local():
            return ModalDictRunStore()
    except Exception:
        pass
    return InMemoryRunStore()


def find_step_weights(job_dir: Path, step: int, total_steps: int) -> Optional[Path]:
    """Find the .safetensors weights file produced for a given step.

    Matches patterns like:
      - {job_name}_{step}.safetensors
      - {job_name}_step_{step}.safetensors
      - {job_name}_{step:08d}.safetensors
      - For final step: {job_name}.safetensors or adapter.safetensors
    """
    if not job_dir.exists():
        return None

    step_str = str(step)
    step_patterns = [
        re.compile(rf"(^|[_\-\. ]){step_str}([_\-\. ]|$|\.safetensors$)", re.IGNORECASE),
        re.compile(rf"{step:08d}"),
        re.compile(rf"step[-_]?{step_str}", re.IGNORECASE),
    ]

    for file in sorted(job_dir.iterdir()):
        if not file.is_file() or file.suffix.lower() != ".safetensors":
            continue
        if any(pat.search(file.name) or pat.search(file.stem) for pat in step_patterns):
            return file

    # If it is the final step, check for root adapter file names
    if step >= total_steps:
        final_named = job_dir / f"{job_dir.name}.safetensors"
        if final_named.is_file():
            return final_named
        adapter_named = job_dir / "adapter.safetensors"
        if adapter_named.is_file():
            return adapter_named

    return None


def download_dataset(dataset_url: str, destination_dir: Path) -> Path:
    """Download a dataset from an HTTP(S) URL or local path into destination_dir."""
    destination_dir.mkdir(parents=True, exist_ok=True)

    # Local file or file:// URL
    if dataset_url.startswith("file://"):
        local_path = Path(dataset_url[7:])
        if local_path.is_dir():
            return local_path
        target_file = destination_dir / local_path.name
        shutil.copy2(local_path, target_file)
        return target_file
    elif "://" not in dataset_url:
        local_path = Path(dataset_url)
        if local_path.is_dir():
            return local_path
        target_file = destination_dir / local_path.name
        shutil.copy2(local_path, target_file)
        return target_file

    # HTTP(S) URL
    filename = Path(dataset_url.split("?")[0]).name or "dataset.zip"
    target_file = destination_dir / filename

    with httpx.Client(timeout=120.0, follow_redirects=True) as client:
        with client.stream("GET", dataset_url) as response:
            response.raise_for_status()
            with target_file.open("wb") as f:
                for chunk in response.iter_bytes(chunk_size=65536):
                    f.write(chunk)

    return target_file


def format_run_response(data: dict[str, Any]) -> dict[str, Any]:
    """Format run data into the exact GET /run/{id} schema conforming to ADR-0009."""
    now = time.time()
    state = data.get("state", RunState.QUEUED.value)
    started_at = data.get("started_at")

    if state == RunState.RUNNING.value and started_at:
        elapsed = now - started_at
        cost = calculate_cost_usd(elapsed)
    else:
        elapsed = data.get("elapsed_seconds", 0.0)
        cost = data.get("cost_usd", calculate_cost_usd(elapsed))

    return {
        "run_id": data["run_id"],
        "state": state,
        "step": data.get("step", 0),
        "total_steps": data.get("total_steps", 0),
        "elapsed_seconds": round(elapsed, 2),
        "cost_usd": cost,
        "checkpoints": data.get("checkpoints", []),
        "error": data.get("error", None),
    }


def execute_training_run(
    run_id: str,
    dataset_url: str,
    recipe: TrainingRecipe,
    store: RunStore,
    uploader: Optional[R2ArtifactUploader] = None,
    cancel_event: Optional[threading.Event] = None,
    training_executor: Optional[Callable[..., Any]] = None,
) -> None:
    """Execute the end-to-end training pipeline on GPU worker."""
    started_at = time.time()
    uploader = uploader or R2ArtifactUploader()

    # Transition to running
    store.update(
        run_id,
        state=RunState.RUNNING.value,
        started_at=started_at,
        step=0,
        total_steps=recipe.steps,
        error=None,
    )

    work_dir_obj = tempfile.TemporaryDirectory(prefix=f"dungeonforge_run_{run_id}_")
    work_dir = Path(work_dir_obj.name)

    try:
        if cancel_event and cancel_event.is_set():
            _handle_cancellation(run_id, started_at, store)
            return

        # 1. Download dataset
        dataset_download_dir = work_dir / "dataset_download"
        try:
            downloaded = download_dataset(dataset_url, dataset_download_dir)
        except Exception as e:
            error_msg = f"Failed to download dataset from '{dataset_url}': {e}"
            _handle_failure(run_id, started_at, error_msg, store)
            return

        # 2. Extract and validate dataset
        dataset_extract_dir = work_dir / "dataset_extracted"
        try:
            dataset_dir = extract_dataset_if_archive(downloaded, dataset_extract_dir)
            validate_dataset(dataset_dir, trigger_phrase=recipe.trigger_phrase)
        except (DatasetValidationError, TrainingError) as e:
            error_msg = f"Dataset validation failed: {e}"
            _handle_failure(run_id, started_at, error_msg, store)
            return
        except Exception as e:
            error_msg = f"Unexpected dataset preparation error: {e}"
            _handle_failure(run_id, started_at, error_msg, store)
            return

        if cancel_event and cancel_event.is_set():
            _handle_cancellation(run_id, started_at, store)
            return

        # 3. Build ai-toolkit configuration
        output_dir = work_dir / "output"
        job_dir = output_dir / run_id
        job_dir.mkdir(parents=True, exist_ok=True)
        samples_dir = job_dir / "samples"
        samples_dir.mkdir(parents=True, exist_ok=True)

        config_dict = build_aitoolkit_config(
            dataset_path=dataset_dir,
            recipe=recipe,
            job_name=run_id,
            output_dir=str(output_dir),
            validate=False,  # Already validated above
        )
        config_path = work_dir / f"{run_id}.yaml"
        config_str = emit_config(
            dataset_path=dataset_dir,
            recipe=recipe,
            job_name=run_id,
            output_dir=str(output_dir),
            validate=False,
        )
        config_path.write_text(config_str, encoding="utf-8")

        # 4. Run training
        # Target checkpoint intervals: e.g. 250, 500, ..., up to total_steps
        save_every = recipe.save_every or 250
        checkpoint_steps = list(range(save_every, recipe.steps + 1, save_every))
        if recipe.steps not in checkpoint_steps:
            checkpoint_steps.append(recipe.steps)

        checkpoints_completed: list[dict[str, Any]] = []

        def checkpoint_callback(step: int) -> None:
            """Locate checkpoint files, compose sample grid, and upload atomically to R2."""
            weights_file = find_step_weights(job_dir, step, recipe.steps)
            if not weights_file:
                raise FileNotFoundError(
                    f"Expected weights file for checkpoint step {step} in {job_dir}"
                )

            grid_output = work_dir / f"samples_step_{step}.png"
            grid_file = find_and_compose_step_grid(samples_dir, step, grid_output)
            if not grid_file or not grid_file.is_file():
                # If no samples generated, produce a standardized placeholder grid
                from PIL import Image

                placeholder = Image.new("RGB", (recipe.resolution, recipe.resolution), color=(40, 40, 40))
                placeholder.save(grid_output, format="PNG")
                grid_file = grid_output

            # Atomically upload to R2
            artifact = uploader.upload_checkpoint(
                run_id=run_id,
                step=step,
                weights_path=weights_file,
                sample_grid_path=grid_file,
            )
            checkpoints_completed.append(artifact.to_dict())
            store.update(
                run_id,
                step=step,
                checkpoints=list(checkpoints_completed),
            )

        if training_executor is not None:
            # Custom training executor callback (e.g. for testing / simulation)
            training_executor(
                run_id=run_id,
                config_path=config_path,
                job_dir=job_dir,
                samples_dir=samples_dir,
                checkpoint_steps=checkpoint_steps,
                checkpoint_callback=checkpoint_callback,
                cancel_event=cancel_event,
            )
        else:
            # Standard ai-toolkit execution
            _run_aitoolkit_subprocess(
                config_path=config_path,
                job_dir=job_dir,
                checkpoint_steps=checkpoint_steps,
                checkpoint_callback=checkpoint_callback,
                cancel_event=cancel_event,
            )

        if cancel_event and cancel_event.is_set():
            _handle_cancellation(run_id, started_at, store)
            return

        # 5. Success completion
        finished_at = time.time()
        elapsed = finished_at - started_at
        cost = calculate_cost_usd(elapsed)
        store.update(
            run_id,
            state=RunState.SUCCEEDED.value,
            step=recipe.steps,
            finished_at=finished_at,
            elapsed_seconds=round(elapsed, 2),
            cost_usd=cost,
            checkpoints=checkpoints_completed,
            error=None,
        )

    except Exception as e:
        logger.exception("Training run %s failed with exception: %s", run_id, e)
        _handle_failure(run_id, started_at, str(e), store)
    finally:
        try:
            work_dir_obj.cleanup()
        except Exception:
            pass


def _run_aitoolkit_subprocess(
    config_path: Path,
    job_dir: Path,
    checkpoint_steps: list[int],
    checkpoint_callback: Callable[[int], None],
    cancel_event: Optional[threading.Event],
) -> None:
    """Execute ai-toolkit CLI as a subprocess and monitor checkpoint outputs."""
    # Locate run.py
    run_py_candidates = [
        Path("/opt/ai-toolkit/run.py"),
        Path("ai-toolkit/run.py"),
        Path("run.py"),
    ]
    run_py = next((p for p in run_py_candidates if p.is_file()), None)
    if not run_py:
        # Fallback to python module or command
        cmd = ["python3", "-m", "toolkit.job", str(config_path)]
    else:
        cmd = ["python3", str(run_py), str(config_path)]

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    processed_steps: set[int] = set()

    while proc.poll() is None:
        if cancel_event and cancel_event.is_set():
            proc.terminate()
            try:
                proc.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                proc.kill()
            return

        # Poll for new checkpoint steps
        for step in checkpoint_steps:
            if step not in processed_steps:
                weights = find_step_weights(job_dir, step, checkpoint_steps[-1])
                if weights:
                    checkpoint_callback(step)
                    processed_steps.add(step)

        time.sleep(1.0)

    # Process finished, capture returncode & output
    stdout, stderr = proc.communicate()
    if proc.returncode != 0:
        error_details = stderr.strip() or stdout.strip() or f"Process exited with code {proc.returncode}"
        raise TrainingError("aitoolkit_failed", f"ai-toolkit failed (code {proc.returncode}): {error_details}")

    # Process any final remaining checkpoints
    for step in checkpoint_steps:
        if step not in processed_steps:
            weights = find_step_weights(job_dir, step, checkpoint_steps[-1])
            if weights:
                checkpoint_callback(step)
                processed_steps.add(step)


def _handle_cancellation(run_id: str, started_at: float, store: RunStore) -> None:
    now = time.time()
    elapsed = now - started_at
    store.update(
        run_id,
        state=RunState.CANCELLED.value,
        finished_at=now,
        elapsed_seconds=round(elapsed, 2),
        cost_usd=calculate_cost_usd(elapsed),
    )


def _handle_failure(run_id: str, started_at: float, error_message: str, store: RunStore) -> None:
    now = time.time()
    elapsed = now - started_at
    store.update(
        run_id,
        state=RunState.FAILED.value,
        finished_at=now,
        elapsed_seconds=round(elapsed, 2),
        cost_usd=calculate_cost_usd(elapsed),
        error=error_message,
    )


class LocalRunnerManager:
    """Manages asynchronous local worker threads and cancellation events."""

    def __init__(
        self,
        store: Optional[RunStore] = None,
        training_executor: Optional[Callable[..., Any]] = None,
    ) -> None:
        self.store = store or get_default_run_store()
        self.training_executor = training_executor
        self._threads: dict[str, threading.Thread] = {}
        self._cancel_events: dict[str, threading.Event] = {}
        self._lock = threading.Lock()

    def spawn(
        self,
        run_id: str,
        dataset_url: str,
        recipe: TrainingRecipe,
        uploader: Optional[R2ArtifactUploader] = None,
        training_executor: Optional[Callable[..., Any]] = None,
    ) -> None:
        with self._lock:
            cancel_event = threading.Event()
            self._cancel_events[run_id] = cancel_event

            thread = threading.Thread(
                target=execute_training_run,
                kwargs={
                    "run_id": run_id,
                    "dataset_url": dataset_url,
                    "recipe": recipe,
                    "store": self.store,
                    "uploader": uploader,
                    "cancel_event": cancel_event,
                    "training_executor": training_executor or self.training_executor,
                },
                daemon=True,
            )
            self._threads[run_id] = thread
            thread.start()

    def cancel(self, run_id: str) -> bool:
        with self._lock:
            event = self._cancel_events.get(run_id)
            if event:
                event.set()
                return True
            return False

    def is_running(self, run_id: str) -> bool:
        with self._lock:
            t = self._threads.get(run_id)
            return t is not None and t.is_alive()
