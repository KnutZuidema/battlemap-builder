---
status: accepted
---

# Training Run lifecycle, Dataset Snapshots and LoRA Adapter publication

A `Training Run` is a Curator-owned entity persisted one directory per run (`data/training/<run_id>/run.json` plus an appended `metrics.jsonl`) whose only states are `queued`, `running`, `succeeded`, `failed` and `cancelled`: Modal owns the compute states and Curator owns publication, so a `LoRA Adapter` is a separate immutable entity (`data/adapters/<adapter_id>/adapter.json`) linked to its run by id rather than a run state a human has to reach. A run's dataset is a frozen `Dataset Snapshot` (ZIP + content digest in R2, recorded on the run) that the freeze endpoint produces only while `GET /api/dataset/readiness` reports the scope `ready: true`, and the run records the exact `Training Recipe` it dispatched — that same document *is* the Modal job request body, so a historical run replays field for field.

## Endpoints

| Method | Path | Purpose |
| :--- | :--- | :--- |
| `GET` \| `POST` | `/api/training/runs` | List runs; create one in `queued` with a frozen dataset + `Training Recipe` |
| `GET` | `/api/training/runs/{id}` | Run record: state, recipe, dataset, artifact states, checkpoint samples, cost |
| `POST` | `/api/training/runs/{id}/dispatch` | Start a `queued` run; refused while another run is `running` |
| `POST` | `/api/training/runs/{id}/cancel` | Forward cancellation; non-terminal runs only |
| `POST` | `/api/training/runs/{id}/publish` | `{step, scale}` → adapter descriptor from that `Checkpoint` |
| `POST` \| `GET` | `/api/training/datasets` | Freeze a snapshot; list snapshots |
| `GET` \| `DELETE` | `/api/training/datasets/{id}` | Snapshot detail; delete only while no run references it |
| `GET` | `/api/training/adapters` | Adapter descriptors (retired included, marked) |
| `GET` | `/api/training/adapters/{id}` | One descriptor |
| `POST` | `/api/training/adapters/{id}/retire` | Soft-retire (unselectable); `{"retired": false}` restores |
| `GET` | `/api/models` | The derived model registry (ADR-0010) |

## Decision

- **Dispatch is explicit and serialized.** `POST /api/training/runs` only persists `queued`; `POST …/dispatch` starts it, and one active run at a time is enforced because the L40S tier is a single GPU and a run is ~40 minutes. A refused dispatch leaves the record `queued` with the conflict reported.
- **Progress is polled and survives restarts.** A background poller follows a `running` run at ~15s backing off, appending metric snapshots to `metrics.jsonl`. Modal being unreachable keeps the run `running`, recording `last_polled_at` and the last error; a remote job that no longer exists terminates it as `failed` with a `lost_remote` detail. On boot every non-terminal run is re-polled and resumed.
- **Artifacts live in R2 only, under a public-read `artifacts/` prefix.** Modal uploads the dataset ZIP, the `.safetensors` and a sample grid per `Checkpoint`; Curator never serves them from the pod (no public ingress, and `/data` is a 20 GiB volume shared with staging and curated).
- **Every artifact is `pending`, `verified` or `mismatch`.** Verification is an R2 `HEAD` comparing byte size and, when the upload declared one, the object's SHA-256 checksum; `succeeded` requires the adapter artifact `verified` while sample grids may still be `pending`. An object uploaded without a declared checksum is accepted on size alone, and `POST /api/training/runs/{id}/verify` exists as the escape hatch that downloads and hashes it.
- **Publication is an explicit operator pick.** The run record lists the sample grid of every `Checkpoint`; `POST …/publish {step, scale}` writes an immutable descriptor (R2 URL, SHA-256, step, scale, Base Model, recipe digest, dataset digest, run id) and rejects a step outside the listed samples. The adapter id is `{run_id}-s{step}`, so re-publishing a step is a conflict rather than a duplicate artifact. A run that the curator declines to publish stays `succeeded` with no adapter.
- **Adapters are retired, never deleted.** A retired adapter disappears from the registry but keeps resolving for recorded benchmark results and cost entries; the R2 object is never removed.
- **Training spend joins generation spend.** The ledger's `history` becomes a tagged `CostRecord` union (`generation`, `training`), migrated once on load and persisted, so one HUD total and per-entry amortization come from `/api/cost/stats`.
- **The `Trigger Phrase` becomes a per-run field and the constant moves to the ADR-0008 value** (`dungeonforge battlemap`), so a run trained on legacy captions stays honest about the phrase its samples used.
- **The Modal contract is a `training/` FastAPI app with three routes** (`POST /run`, `GET /run/{id}`, `POST /run/{id}/cancel`) behind a bearer token, speaking the run's `Training Recipe` verbatim. `CURATOR_MODAL_ENDPOINT` / `CURATOR_MODAL_TOKEN` absent ⇒ `/api/training/*` fails loud with 503, matching a missing `OPENROUTER_API_KEY`.

## Considered options

- **Mirroring Modal's state verbatim** — no `running`/`succeeded` distinction Curator can act on, and the record becomes wrong the moment Modal prunes the job.
- **An `awaiting_publish` state, or auto-publishing the final `Checkpoint`** — the first lets a run sit non-terminal forever because a human declined to publish; the second puts an adapter nobody chose into the registry and lets the Benchmark Drawer select untrained-by-intent weights.
- **Modal calling back into Curator** — Curator is ClusterIP-only with no ingress, and polling already survives rollouts.
- **Freezing the dataset inline in run creation, or mutating `GET /api/dataset/export` with an upload** — a retried dispatch would re-upload, and a plain download would be able to publish artifacts.
- **Curator downloading every artifact to hash it, or keeping a local copy** — a ~400 MB transfer through the pod per artifact, against the same volume that holds staging and curated.
- **Presigned GET URLs for Modal and the inference host** — a descriptor recorded for a benchmark that will be judged months later must still resolve; presigned URLs expire.
- **Blocking on nothing: a training-local object store seam** — deferred in favour of the full S3 adapter on the decomposed ports, so object storage has one implementation and one layout.

## Consequences

- Implementation is blocked behind the port decomposition (`CatalogRepository` / `BlobStorage`) and the R2 `S3StorageAdapter`; `Training Run`s, `Dataset Snapshot`s and adapter descriptors arrive as catalog entities addressed by typed `BlobKey`s, and `curator/docs/storage_s3_r2_plan.md` (stale: theme-nested curated paths, star ratings, `rejections.json`, `_lora1024` files) is deleted rather than corrected.
- The digest in `adapter.json` is a claim whenever the uploader declared no checksum: integrity is size-only for those objects.
- The `Trigger Phrase` shrinking to two tokens breaks `curator/tools/lora-viewer.html`'s hardcoded five-token prefix highlight, and ZIPs exported before the change stay captioned with the old phrase.
- Amortized cost per generated `Battlemap` (the metric the rollout verdict rests on) becomes computable per registry entry.
- The Benchmark Drawer enumerates adapters, not runs, so a successful run with no publication is invisible there — by design.
