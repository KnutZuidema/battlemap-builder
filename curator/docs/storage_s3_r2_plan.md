# Storage Architecture Plan: S3 & Cloudflare R2 Storage Readiness

## 1. Executive Summary & Decoupling Audit

The Curator service utilizes a Hexagonal Architecture (Ports and Adapters) pattern to isolate domain models and business logic from persistence mechanisms. ADR-0006 replaced the former monolithic `StorageRepository` with two focused, asynchronous ports in `curator/src/ports/`:

- `CatalogRepository` (`catalog_repository.rs`) persists domain entities (`Candidate`, `CuratedMap`, rejection records).
- `BlobStorage` (`blob_storage.rs`) performs raw byte I/O addressed by strongly-typed `BlobKey` values.

### Port Audit

An audit of both ports confirms complete decoupling from local filesystem paths:
- **No Path Leaks:** Neither port exposes `std::path::Path`, `std::path::PathBuf`, or OS-specific filesystem handles. `BlobKey::to_path` is the single source of truth for the canonical storage-relative path, so no caller hand-builds one.
- **Abstract Identifiers:** All blob access is addressed by `BlobKey` (`StagingLayer`, `CuratedLayer`, `ExportArchive`, `RejectionLog`, `TelemetryLog`, and the directory prefixes); catalog access operates strictly on domain primitives:
  - Candidate and map identifiers: `&str`
  - Classification and filtering: `CuratedFilter::theme` (`&Theme`/`Theme`)
  - Layer indexing: `usize`
  - In-memory byte buffers: `&[u8]`, `Vec<u8>`
  - Domain records: `&Candidate`, `&CuratedMap`
- **LoRA Dataset Packaging:** `DatasetExportService::export_lora_dataset` packages per-Map-Section 1024×1024 PNGs and captions into a standard ZIP archive in memory, never relying on temporary disk files.
- **Atomic Abstractions:** Curation, rejection, layer/label edits, and staging cleanup are modeled as application services (`CurationService`, `LibraryService`, `StorageMaintenanceService`) orchestrating the two ports, rather than the adapter exposing filesystem moves or file unlinks.

---

## 2. Object Key Mapping Strategy

When transitioning from the local `FilesystemBlobStorage`/`FilesystemCatalogRepository` adapters to cloud object storage backends, flat hierarchies with virtual prefix delimiters (`/`) map directly to S3/R2 key namespaces. `BlobKey::to_path` already emits exactly these keys.

### 2.1 Staging Area (`staging/`)

Incoming battlemaps harvested from Reddit, generated via benchmark suites, or uploaded manually reside in one uniform `staging/{id}/` bundle layout. The ingestion source lives on the `Candidate` record; there are no `staging/reddit/` or `staging/generated/` path prefixes.

| Entity | Object Key Format | Content-Type | Example |
| :--- | :--- | :--- | :--- |
| **Candidate Manifest** | `staging/{id}/manifest.json` | `application/json` | `staging/cand_981a/manifest.json` |
| **Layer Image (Primary)** | `staging/{id}/{index}.png` | `image/png` | `staging/cand_981a/0.png` |
| **Multi-Layer Image** | `staging/{id}/{index}.png` | `image/png` | `staging/cand_981a/1.png` |

#### Atomic Move Semantics in Object Storage
Object stores lack a native POSIX `rename` syscall, so curation is a saga over the ports rather than a single move:
1. `BlobStorage::write_blob` for every included layer at `curated/{id}/{index}_{role}.png` (rollback deletes exactly the blobs this call created on failure)
2. `CatalogRepository::save_curated` commits the finalized `curated/{id}/manifest.json`
3. `BlobStorage::list_prefix` + `delete_blob` prune the source keys in `staging/{id}/`, then `CatalogRepository::delete_candidate` removes the candidate

### 2.2 Curated Dataset Library (`curated/`)

Approved battlemaps live in flat `<id>/` bundles (ADR-0003); theme membership is derived from the map's tags, never from a directory prefix. LoRA training samples are derived per Map Section on demand at export time (ADR-0007), so no `<id>_lora1024` companions are stored:

| Entity | Object Key Format | Content-Type | Description |
| :--- | :--- | :--- | :--- |
| **Curated Manifest** | `curated/{id}/manifest.json` | `application/json` | Full `CuratedMap` metadata (tags, layers, sections) |
| **Layer PNGs** | `curated/{id}/{index}_{role}.png` | `image/png` | Layer files (e.g. `0_ground.png`, `1_upper_1.png`) |

### 2.3 Catalog & Records (`rejected/`, `telemetry/`)

| Entity | Object Key Format | Content-Type | Description |
| :--- | :--- | :--- | :--- |
| **Rejection Log** | `rejected/rejections.json` | `application/json` | Append-only catalog of rejected map candidate hashes |
| **Cost Ledger** | `telemetry/cost_ledger.json` | `application/json` | Model token generation and inference spend ledger |

---

## 3. Provider Analysis & Compatibility

### 3.1 Cloudflare R2 (Recommended Primary Target)

Cloudflare R2 is the optimal cloud storage tier for the Battlemap Curator:
- **Zero Egress Fees:** Eliminates egress bandwidth costs when streaming large 2048x2048 PNGs to web triage clients and downloading multi-gigabyte LoRA dataset ZIP archives.
- **S3 API Compatibility:** Implements the core Amazon S3 REST API (SigV4 authentication, `GetObject`, `PutObject`, `DeleteObjects`, `ListObjectsV2`, `UploadPart`).
- **Edge Caching:** Directly routes through Cloudflare's global edge network via custom domain bindings (e.g. `https://assets.battlemapbuilder.com/curated/...`).

### 3.2 AWS S3

- **Read-After-Write Consistency:** Strong consistency for `PUT` and `DELETE` requests in all regions ensures immediate visibility of newly curated assets.
- **Intelligent-Tiering:** Automatically moves historical curated themes to cold archive tiers when inactive.
- **IAM Roles & Workload Identity:** Seamless EC2 / ECS / EKS role attachment without long-lived static secrets.

### 3.3 MinIO (Local & Self-Hosted Development)

- **100% S3 Compliance:** Runs as a single container or local binary for offline development, local CI testing, and air-gapped deployments.
- **Parity with Production:** Developers run `minio server /data` locally, using identical `S3StorageAdapter` code paths as production Cloudflare R2.

---

## 4. Lifecycle Policies & Bucket Management

### 4.1 Staging Prune Policy (`staging/`)
- **Retention Limit:** 14-day expiration on all objects under `staging/`.
- **Incomplete Multipart Uploads:** Abort incomplete multipart uploads after 3 days to avoid dangling storage charges.

### 4.2 Curated Library Retention (`curated/`)
- **Durability:** Standard storage tier with optional transition to Cloudflare R2 standard / S3 Standard-IA after 90 days.
- **Versioning:** Object versioning enabled on `curated/` to guard against accidental deletion during bulk re-tagging or automated cleanup scripts.

---

## 5. Multipart Uploads & Large Object Transfer

Battlemap PNG files typically measure 2048x2048 to 4096x4096 (5MB to 40MB per layer bundle).

1. **Threshold:**
   - Single-part `PutObject` for files `< 8 MB`.
   - Multipart Upload (`CreateMultipartUpload` -> `UploadPart` -> `CompleteMultipartUpload`) for payloads `>= 8 MB` with 5MB part sizes.
2. **Concurrency:**
   - Layer uploads within a bundle run concurrently using `tokio::task::JoinSet`.
3. **Dataset Export Streaming:**
   - LoRA dataset export (`export_lora_dataset`) generates standard DEFLATE-compressed ZIP archives in memory.
   - For datasets exceeding 100MB, streaming responses via Axum's `Body::from_stream` directly transfer the byte chunks without retaining full archive buffers in RAM.

---

## 6. Implementation Architecture

```
                      +-----------------------------+
                      |   Domain / Application      |
                      +-----------------------------+
                                     |
                +--------------------+--------------------+
                |                                         |
                v                                         v
   +-------------------------+              +-------------------------+
   |  CatalogRepository      |              |      BlobStorage        |
   |        (Port)           |              |         (Port)          |
   +-------------------------+              +-------------------------+
                |                                         |
                v                                         v
   +-------------------------+              +-------------------------+
   | FilesystemCatalogRepo   |              |  FilesystemBlobStorage  |
   |   (Local disk adapter)  |              |   (Local disk adapter)  |
   +-------------------------+              +-------------------------+
                |                                         |
                v                                         v
         Local File System                         AWS S3 / R2 API
     staging/{id}/manifest.json                https://<account>.r2.cloudflarestorage.com
     curated/{id}/manifest.json
```

A cloud deployment swaps only the two adapters (`S3CatalogRepository`, `S3BlobStorage`); the ports and every service above them stay unchanged.

### Configuration via Environment Variables

```env
# Storage Provider Selection
STORAGE_BACKEND=s3 # options: filesystem | s3

# S3 / R2 / MinIO Endpoint Settings
S3_ENDPOINT=https://<account_id>.r2.cloudflarestorage.com
S3_BUCKET=battlemap-curator
S3_REGION=auto
S3_ACCESS_KEY_ID=r2_token_id
S3_SECRET_ACCESS_KEY=r2_token_secret
S3_PUBLIC_URL_PREFIX=https://cdn.battlemapbuilder.com
```
