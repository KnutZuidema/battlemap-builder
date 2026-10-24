# Curator Deployment & Operations Runbook

This guide covers cluster provisioning, continuous deployment, local access via port-forwarding, and operational verification for the **Curator** service.

---

## 1. Overview & Architecture

Curator is an internal ingestion, benchmarking, and curation platform for tabletop RPG battlemaps. It runs as a containerized Rust service deployed to a private Kubernetes cluster.

### Key Architectural Pillars

- **Hardware & Node Topology**: Deployed on a **Hetzner Cloud ARM64 k0s cluster** utilizing `cax11` worker nodes (Ampere Altra ARM64 compute).
- **Internal-Only Networking**: Exposed strictly via a **`ClusterIP` service on port `3002`**. There are intentionally **no public IngressRoutes, Traefik routes, or DNS records**. The triage interface and API keys are completely shielded from public exposure.
- **Persistent Storage**: Backed by **Hetzner Cloud Volumes CSI** (`hcloud-volumes` storage class, `20Gi`, `ReadWriteOnce`). The volume mounts to `/data` in the container.
- **Volume Recreate Strategy**: The deployment specifies `strategy.type: Recreate`. Because Hetzner Cloud block volumes are `ReadWriteOnce` and cannot be mounted across multiple nodes simultaneously, `Recreate` guarantees the old pod releases the volume and terminates before the replacement pod starts and claims the mount.
- **Filesystem Permissions**: A lightweight `initContainers` (`busybox:1.36`) runs `chown -R 1000:1000 /data` prior to application startup, ensuring the unprivileged container user (`uid: 1000`, `gid: 1000`, `fsGroup: 1000`) has full read and write access to the volume root.

---

## 2. One-Time Cluster Prerequisites

Before the automated CD pipeline or manual deployment can succeed, perform these one-time provisioning steps on the cluster.

### 2.1. Create Namespace

Provision the dedicated `curator` namespace:

```bash
kubectl apply -f deploy/curator/namespace.yaml
```

*(Or from within the `deploy/curator` directory: `kubectl apply -f namespace.yaml`)*

### 2.2. Configure Application Secrets (`curator-secrets`)

Curator requires credentials for model benchmarking, vision descriptions, gated model weights, cloud GPU training runs, R2 artifact storage, and serverless LoRA inference.

#### Automated Interactive Setup (Recommended)

Run the interactive provisioning wizard to walk step-by-step through generating keys, writing local `curator/.env`, creating `deploy/curator/secret.yaml`, verifying API connectivity, and applying the secret to your Kubernetes cluster:

```bash
./scripts/provision-credentials.sh
```

#### Manual Configuration

1. Copy the secret template:
   ```bash
   cp deploy/curator/secret.yaml.template deploy/curator/secret.yaml
   ```

2. Edit `deploy/curator/secret.yaml` and configure credentials:
   - `OPENROUTER_API_KEY`: API key for benchmark maps, theme suggestions, and vision descriptions.
   - `HF_TOKEN`: Hugging Face User Access Token (read) for downloading gated `flux-3-image-klein-base` weights.
   - `MODAL_TOKEN_ID` & `MODAL_TOKEN_SECRET`: Modal credentials for GPU training runners.
   - `CURATOR_MODAL_ENDPOINT` & `CURATOR_MODAL_TOKEN`: URL and bearer auth token for the training runner FastAPI service.
   - `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`: Cloudflare R2 S3 API credentials.
   - `R2_BUCKET_NAME`: R2 bucket for dataset snapshots and adapter artifacts (e.g., `dungeonforge-artifacts`).
   - `R2_ENDPOINT` & `R2_PUBLIC_URL`: S3 endpoint and public HTTPS base URL for artifact downloads.
   - `WAVESPEED_API_KEY`: WaveSpeedAI API key for serverless FLUX 3 Klein LoRA inference.
   - `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_REFRESH_TOKEN`, `REDDIT_USER_AGENT`: Reddit harvesting credentials.

3. Apply the secret to the cluster:
   ```bash
   kubectl apply -f deploy/curator/secret.yaml
   ```

> **Git Hygiene Notice**: `deploy/curator/secret.yaml` is ignored in `.gitignore`. **Never commit `secret.yaml` to git.** Only `secret.yaml.template` is tracked in the repository with placeholder values.

### 2.3. Configure GitHub Container Registry (GHCR) Image Pull Secret

Because Curator images are stored in a private/restricted GitHub Packages registry (`ghcr.io/knutzuidema/curator`), Kubernetes requires an image pull secret named `ghcr-secret`.

- **Automated by CD Pipeline**: When Continuous Deployment runs via GitHub Actions, the workflow automatically creates or updates `ghcr-secret` in the `curator` namespace using the persistent `GHCR_TOKEN` secret.
- **Manual Provisioning (for manual deploys or troubleshooting)**:
  ```bash
  kubectl create secret docker-registry ghcr-secret \
    --namespace=curator \
    --docker-server=ghcr.io \
    --docker-username=<GITHUB_USERNAME> \
    --docker-password=<GITHUB_PERSONAL_ACCESS_TOKEN_WITH_READ_PACKAGES> \
    --dry-run=client -o yaml | kubectl apply -f -
  ```

### 2.4. Configure GitHub Actions Repository Secret (`KUBECONFIG`)

To enable automated cluster deployment from GitHub Actions:

1. Navigate to your repository on GitHub: **Settings** &rarr; **Secrets and variables** &rarr; **Actions**.
2. Click **New repository secret**.
3. Name: `KUBECONFIG`.
4. Value: Paste the contents of your cluster's kubeconfig file.
5. Save the secret.

> **Safety Guard**: The CD workflow checks `if: ${{ secrets.KUBECONFIG != '' }}`. If `KUBECONFIG` is not configured, the container image will still be built and published to GHCR, but the cluster rollout step will be skipped safely without failing the job.

### 2.5. Configure GitHub Actions Repository Secret (`GHCR_TOKEN`)

Because the default `GITHUB_TOKEN` expires after 1 hour, a persistent Personal Access Token is used to populate `ghcr-secret` so that pod restarts or node rescheduling can always pull images without encountering `403 Forbidden`:

1. Generate a GitHub Personal Access Token (classic with `read:packages` scope, or fine-grained PAT with read access to the `knutzuidema/curator` package).
2. Navigate to your repository on GitHub: **Settings** &rarr; **Secrets and variables** &rarr; **Actions**.
3. Click **New repository secret**.
4. Name: `GHCR_TOKEN`.
5. Value: Paste your Personal Access Token.
6. Save the secret.

---

## 3. Continuous Deployment (CD) Flow

The deployment pipeline is defined in `.github/workflows/curator-cd.yml` and manages automated builds and zero-downtime cluster rollouts.

```
git push main (curator/**, deploy/curator/**)
    │
    ▼
Docker Buildx (linux/arm64)
    │
    ▼
Push to GHCR (ghcr.io/knutzuidema/curator:<sha> + :latest)
    │
    ▼
Check secrets.KUBECONFIG ────(Not Set)───► Safe Exit (Image published)
    │ (Set)
    ▼
Apply Base Manifests (namespace, pvc, service)
    │
    ▼
Refresh ghcr-secret from GHCR_TOKEN
    │
    ▼
Deploy with Immutable SHA Tag (kubectl set image)
    │
    ▼
Rollout Verification (kubectl rollout status)
```

### Pipeline Steps

1. **Trigger Condition**:
   - Triggers on push to `main` when paths change under `curator/**`, `deploy/curator/**`, or `.github/workflows/curator-cd.yml`.
   - Can also be triggered manually using `workflow_dispatch`.
2. **Multi-Stage Build for ARM64**:
   - Uses Docker Buildx with `--platform=$BUILDPLATFORM` and native cross-linking to compile for `linux/arm64` without QEMU CPU emulation, targeting Hetzner `cax11` worker nodes.
   - Builds the Rust application and embeds UI assets into a minimal container.
   - Pushes both `ghcr.io/knutzuidema/curator:<sha>` (immutable release tag) and `ghcr.io/knutzuidema/curator:latest`.
   - Utilizes GitHub Actions cache (`type=gha`) for fast incremental Cargo compilation.
3. **Cluster Rollout**:
   - Extracts `KUBECONFIG` and installs `kubectl`.
   - Applies core manifests (`namespace.yaml`, `pvc.yaml`, `service.yaml`).
   - Automatically synchronizes `ghcr-secret` with the configured `GHCR_TOKEN` secret.
   - Dynamically points the deployment to the immutable commit SHA tag:
     ```bash
     IMAGE_TAG="ghcr.io/knutzuidema/curator:${{ github.sha }}"
     kubectl set image -f deploy/curator/deployment.yaml curator="${IMAGE_TAG}" --local -o yaml | kubectl apply -f -
     ```
   - Verifies deployment progress with a 180-second timeout:
     ```bash
     kubectl rollout status deployment/curator -n curator --timeout=180s
     ```

---

## 4. Local Access via Port-Forwarding

Because Curator has no public ingress or domain name, access is established securely from an authorized local machine via Kubernetes port-forwarding.

### 4.1. Start Port-Forward

From the repository root or `deploy/curator` directory:

```bash
# Using just (recommended)
just port-forward

# Or using kubectl directly
kubectl port-forward service/curator 3002:3002 -n curator
```

Keep this process running in a terminal session while interacting with Curator.

### 4.2. Open Application in Browser

Once port-forwarding is established:

- **Web Triage UI**: [http://localhost:3002/](http://localhost:3002/)
  - Canvas viewer with pan/zoom.
  - Hotkey triage (1–6 rating, thematic categorization, rejection logging).
  - Reddit Harvester drawer.
  - Multi-model OpenRouter Benchmark drawer.
- **Health & Status API**: [http://localhost:3002/api/status](http://localhost:3002/api/status)
  - Returns JSON status, uptime, candidate staging queue count, and total curated count.
- **Staging Queue API**: [http://localhost:3002/api/staging](http://localhost:3002/api/staging)
  - Lists pending map candidates in staging with associated metadata.

---

### 4.3. Publishing and Deploying from a Local Machine

The root `justfile` runs the CD pipeline from your machine: **every deployment uses an image published to GHCR**, because that registry is where the cluster nodes pull from. Nothing is built into a local container; the nodes pull the published tag.

```bash
just curator-login          # one-time GHCR login using the gh CLI token (write:packages)
just curator-deploy         # publish this commit (linux/arm64) + roll the cluster onto it
just curator-publish        # publish only: build + push, no rollout
just curator-rollout <tag>  # roll the cluster onto an already published tag
just port-forward           # forwarded access to the running service on localhost:3002
just cluster status         # any deploy/curator/justfile recipe: status, logs, tail-logs, delete
```

- **Tag**: `curator-publish`, `curator-rollout` and `curator-deploy` default to `CURATOR_IMAGE_TAG`, which defaults to the current commit (`git rev-parse HEAD`) — the same immutable `${{ github.sha }}` scheme CD deploys. `CURATOR_IMAGE_TAG=latest just curator-rollout` rolls onto the released tag.
- **Platform**: the nodes are Hetzner `cax11` (arm64) and CI publishes `linux/arm64` only, so that is the default here too (`CURATOR_PLATFORMS` overrides). The Dockerfile cross-compiles that leg natively on an amd64 machine — no qemu involved.
- **Build cache**: the Dockerfile keeps cargo's target dir in a BuildKit cache mount, so the compiled dependency tree survives across builds (every worktree, every commit): a republish after a source change only recompiles the curator crate instead of the whole dependency tree. The mount lives in the builder's cache — reclaim it with `docker buildx prune`.
- **Guards**: `curator-rollout` reads the tag back from GHCR first and refuses to touch the cluster when it is missing/unreadable or carries no `linux/arm64` leg; `curator-publish` verifies the pushed index contains every platform it was asked for.
- **Rollout**: identical to the CD step — `kubectl set image -f deploy/curator/deployment.yaml curator=ghcr.io/knutzuidema/curator:<tag> --local -o yaml | kubectl apply -f -`, then `kubectl rollout status deployment/curator -n curator --timeout=180s`. Cluster prerequisites (namespace, PVC, `curator-secrets`, `ghcr-secret`) come from §2.
- **Private package**: `ghcr.io/knutzuidema/curator` needs credentials, hence `curator-login` (or `docker login ghcr.io -u <user> -p <PAT with read:packages>`). Pods use the existing `ghcr-secret`.
- **Watching the result**: `just port-forward` plus http://localhost:3002/ (§4.1–4.2), or `just cluster tail-logs`.

---

## 5. Operations & Troubleshooting

The `deploy/curator/justfile` provides standard commands for day-to-day cluster operations.

### 5.1. Resource Status

Check deployment, pod, service, and persistent volume claim statuses:

```bash
just status
# Equivalent to: kubectl get all,pvc -n curator
```

Expected healthy output:
- **Pod**: `pod/curator-<hash> 1/1 Running`
- **Service**: `service/curator ClusterIP ... 3002/TCP`
- **Deployment**: `deployment.apps/curator 1/1`
- **PersistentVolumeClaim**: `curator-data-pvc Bound ... 20Gi ReadWriteOnce hcloud-volumes`

### 5.2. Inspecting Logs

```bash
# View recent pod logs
just logs

# Stream real-time logs
just tail-logs

# Inspect init-container logs (if permissions fail)
kubectl logs -n curator deployment/curator -c init-permissions
```

### 5.3. Data Volume Layout & Persistence Verification

All persistent state is stored under `/data` on the Hetzner CSI block volume.

#### Directory Layout

| Path | Purpose |
|------|---------|
| `/data/staging/<id>/` | One uniform bundle per uncurated candidate: `manifest.json` plus layer images (`0.png`, `1.png`, …). Harvested Reddit, benchmark-generated, and manually added candidates all use this layout; the source lives on the manifest, not in a path prefix. |
| `/data/curated/<id>/` | One flat bundle per curated battlemap: `manifest.json` plus deterministically named layers (`0_ground.png`, `1_upper_1.png`, …). Theme membership is derived from the manifest's tags, so no per-theme directories exist. |
| `/data/rejected/rejections.json` | JSON audit trail of discarded candidates with reasons. |
| `/data/telemetry/cost_ledger.json` | Persistent ledger recording token usage, model generation costs, and API spend. |

#### Persistence Verification Procedure

To verify that data survives across pod restarts and recreations on Hetzner CSI storage:

1. **Verify Volume Mount and State**:
   ```bash
   kubectl exec -it -n curator deployment/curator -- ls -la /data
   ```
2. **Note Current Counts**:
   Check `http://localhost:3002/api/status` or run:
   ```bash
   kubectl exec -n curator deployment/curator -- wget -qO- http://localhost:3002/api/status
   ```
3. **Simulate Pod Failure / Rollout**:
   Delete the active pod to trigger recreation:
   ```bash
   kubectl delete pod -l app=curator -n curator
   ```
4. **Watch Replacement Rollout**:
   ```bash
   kubectl get pods -n curator -w
   ```
   Kubernetes terminates the old pod, cleanly detaches the Hetzner volume, initializes permissions with the init-container, and starts the replacement pod.
5. **Verify Data Retention**:
   After the replacement pod transitions to `Running`:
   ```bash
   kubectl exec -it -n curator deployment/curator -- ls -la /data/telemetry /data/staging
   ```
   Confirm that all staged maps and `cost_ledger.json` remain intact and populated.

---

## 6. Common Issues & Resolutions

### Pod in `ImagePullBackOff` or `ErrImagePull`
- **Cause**: Kubernetes cannot authenticate with GHCR to pull private image `ghcr.io/knutzuidema/curator`.
- **Resolution**: Verify that `ghcr-secret` exists in the `curator` namespace:
  ```bash
  kubectl get secret ghcr-secret -n curator
  ```
  If missing or expired, re-create it following section **2.3**.

### Volume Multi-Attach / Attachment Timeout
- **Cause**: Hetzner Cloud Volumes only support single-node attachment (`ReadWriteOnce`).
- **Resolution**: Ensure `strategy.type: Recreate` is set in `deployment.yaml`. Do not use `RollingUpdate` for single-replica RWO volume workloads.

### Pod in `CrashLoopBackOff` with Permission Denied on `/data`
- **Cause**: Host directory permissions on the newly attached volume preventing user `1000` from writing.
- **Resolution**: Verify that the `init-permissions` container ran successfully:
  ```bash
  kubectl logs -n curator deployment/curator -c init-permissions
  ```
  The init-container runs as `uid: 0` (root) and executes `chown -R 1000:1000 /data`.

### Reddit Harvest Fails with 401/403
- **Cause**: Missing or invalid Reddit credentials in `curator-secrets`.
- **Resolution**: Check that `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, and `REDDIT_REFRESH_TOKEN` are correctly set in `secret.yaml` and applied to the cluster.
