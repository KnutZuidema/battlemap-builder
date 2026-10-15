# Storage Architecture Plan: S3 & Cloudflare R2 Storage Readiness

## 1. Executive Summary & Decoupling Audit

The Curator service utilizes a Hexagonal Architecture (Ports and Adapters) pattern to isolate domain models and business logic from persistence mechanisms. The primary storage interface is defined by the `StorageRepository` port (`curator/src/ports/storage.rs`).

### StorageRepository Port Audit

An audit of `StorageRepository` confirms complete decoupling from local filesystem paths:
- **No Path Leaks:** No method signatures in `StorageRepository` expose `std::path::Path`, `std::path::PathBuf`, or OS-specific filesystem handles.
- **Abstract Identifiers:** All access methods operate strictly on domain primitives:
  - Candidate and map identifiers: `&str` (e.g. `"map_42"`)
  - Classification and routing: `&Theme` (e.g. `Theme::Dungeon`, `Theme::Cave`)
  - Layer indexing: `usize`
  - In-memory byte buffers: `&[u8]`, `Vec<u8>`
  - Domain records: `&Candidate`, `&CuratedMap`, `&RedditSidecarMetadata`, `&BenchmarkMetadata`
- **LoRA Dataset Packaging:** `export_lora_dataset(&self, theme: Option<&Theme>) -> Result<Vec<u8>, StorageError>` packages normalized 1024x1024 PNGs and companion `.txt` captions into standard ZIP format without relying on temporary disk files.
- **Atomic Abstractions:** Curation (`curate_staging`), rejection (`reject_staging`), and staging cleanup (`purge_staging`) are modeled as high-level atomic domain operations rather than filesystem moves or file unlinks.

---

## 2. Object Key Mapping Strategy

When transitioning from the local `FilesystemStorage` adapter to a cloud object storage backend (`S3StorageAdapter`), flat hierarchies with virtual prefix delimiters (`/`) map directly to S3/R2 key namespaces.

### 2.1 Staging Area (`staging/`)

Incoming battlemaps harvested from Reddit, generated via benchmark suites, or uploaded manually reside in `staging/`.

| Entity | Object Key Format | Content-Type | Example |
| :--- | :--- | :--- | :--- |
| **Candidate Manifest** | `staging/{id}/manifest.json` | `application/json` | `staging/cand_981a/manifest.json` |
| **Layer Image (Primary)** | `staging/{id}/{index}.png` | `image/png` | `staging/cand_981a/0.png` |
| **Multi-Layer Image** | `staging/{id}/{index}.png` | `image/png` | `staging/cand_981a/1.png` |
| **Subfolder Ingestion** | `staging/{subfolder}/{id}/...` | `image/png` | `staging/reddit/t3_xyz/0.png` |

#### Atomic Move Semantics in Object Storage
Since S3 and R2 lack a native POSIX `rename` syscall, moving candidates from `staging/` to `curated/` during triage approval is implemented as:
1. `CopyObject` from `staging/{id}/{file}` to `curated/{theme}/{id}/{dest_file}`
2. `PutObject` writing the finalized `curated/{theme}/{id}/manifest.json`
3. `DeleteObjects` batch-pruning the source keys in `staging/{id}/`

### 2.2 Curated Dataset Library (`curated/`)

Approved battlemaps organized by theme with full multi-layer representations and LoRA training companions:

| Entity | Object Key Format | Content-Type | Description |
| :--- | :--- | :--- | :--- |
| **Theme Manifest** | `curated/{theme}/{id}/manifest.json` | `application/json` | Full `CuratedMap` metadata (rating, layers, tags) |
| **Layer PNGs** | `curated/{theme}/{id}/{index}_{role}_{elev}.png` | `image/png` | Layer files (e.g. `0_ground_0.png`, `1_upper_1.png`) |
| **LoRA Normalized Image** | `curated/{theme}/{id}/{id}_lora1024.png` | `image/png` | Exact 1024x1024 Lanczos3-rescaled training image |
| **LoRA Companion Caption** | `curated/{theme}/{id}/{id}_lora1024.txt` | `text/plain; charset=utf-8` | Kohya / AI-Toolkit comma-delimited caption |

### 2.3 Catalog & Records (`records/`)

| Entity | Object Key Format | Content-Type | Description |
| :--- | :--- | :--- | :--- |
| **Rejection Log** | `records/rejections.json` | `application/json` | Append-only catalog of rejected map candidate hashes |
| **Cost Ledger** | `records/cost_ledger.json` | `application/json` | Model token generation and inference spend ledger |

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
                                     v
                      +-----------------------------+
                      |  StorageRepository (Port)   |
                      +-----------------------------+
                                     |
                +--------------------+--------------------+
                |                                         |
                v                                         v
   +-------------------------+              +-------------------------+
   |    FilesystemStorage    |              |     S3StorageAdapter    |
   |   (Local disk adapter)  |              |   (S3 / R2 / MinIO)     |
   +-------------------------+              +-------------------------+
                |                                         |
                v                                         v
         Local File System                         AWS S3 / R2 API
     data/staging/ & curated/                  https://<account>.r2.cloudflarestorage.com
```

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
