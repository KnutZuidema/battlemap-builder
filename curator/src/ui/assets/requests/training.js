/**
 * The training run requests: the run listing, queue/launch and dispatch, cancel, artifact
 * verification, adapter publication and retirement, and the adapter listing.
 *
 * A dispatch refusal is an ordinary `RequestError`: its `code` (`RUN_ALREADY_ACTIVE`,
 * `MODAL_NOT_CONFIGURED`) and the server's `error` are what the view turns into its
 * operator-facing message.
 */

import { requestJson } from "./http.js";

function runPath(runId, suffix) {
    return `/api/training/runs/${encodeURIComponent(runId)}/${suffix}`;
}

/** Every training run the runner has recorded. */
export function fetchTrainingRuns({ signal } = {}) {
    return requestJson("/api/training/runs", { signal, operation: "Load training runs" });
}

/** Queue a training run on a frozen snapshot. */
export function launchTrainingRun({ snapshotId, policy }, { signal } = {}) {
    return requestJson("/api/training/runs", {
        method: "POST",
        body: { dataset_snapshot_id: snapshotId, policy },
        signal,
        operation: "Launch training run",
    });
}

/** Dispatch a queued run to the Modal GPU runner. */
export function dispatchTrainingRun(runId, { signal } = {}) {
    return requestJson(runPath(runId, "dispatch"), {
        method: "POST",
        signal,
        operation: "Dispatch training run",
    });
}

/** Cancel a run; any in-flight GPU execution is terminated. */
export function cancelTrainingRun(runId, { signal } = {}) {
    return requestJson(runPath(runId, "cancel"), {
        method: "POST",
        signal,
        operation: "Cancel training run",
    });
}

/** Verify a finished run's artifacts are present in blob storage. */
export function verifyTrainingArtifacts(runId, { signal } = {}) {
    return requestJson(runPath(runId, "verify"), {
        method: "POST",
        signal,
        operation: "Verify training artifacts",
    });
}

/** Publish a run's checkpoint as a selectable LoRA adapter. */
export function publishTrainingAdapter(runId, { step, scale }, { signal } = {}) {
    return requestJson(runPath(runId, "publish"), {
        method: "POST",
        body: { step: Number(step), scale },
        signal,
        operation: "Publish LoRA adapter",
    });
}

/** Retire an adapter (hiding it from the benchmark registry) or restore it. */
export function setAdapterRetired(adapterId, retired, { signal } = {}) {
    return requestJson(`/api/training/adapters/${encodeURIComponent(adapterId)}/retire`, {
        method: "POST",
        body: { retired: Boolean(retired) },
        signal,
        operation: retired ? "Retire LoRA adapter" : "Restore LoRA adapter",
    });
}

/** The published LoRA adapters. */
export function fetchTrainingAdapters({ signal } = {}) {
    return requestJson("/api/training/adapters", { signal, operation: "Load training adapters" });
}
