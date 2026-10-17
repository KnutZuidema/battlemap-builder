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

Curator requires API credentials for OpenRouter model benchmarking and Reddit community map harvesting.

1. Copy the secret template:
   ```bash
   cp deploy/curator/secret.yaml.template deploy/curator/secret.yaml
   ```

2. Edit `deploy/curator/secret.yaml` and enter your credentials:
   - `OPENROUTER_API_KEY`: API key used to generate benchmark maps across models.
   - `REDDIT_CLIENT_ID`: Reddit script app client ID for harvesting `/r/battlemaps`.
   - `REDDIT_CLIENT_SECRET`: Reddit script app client secret.
   - `REDDIT_REFRESH_TOKEN`: User OAuth2 refresh token for API access.
   - `REDDIT_USER_AGENT`: User agent identification string (e.g., `DungeonForge-Curator/1.0 (by /u/username)`).

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
| `/data/staging/` | Incoming uncurated candidate images and metadata sidecars. |
| `/data/staging/reddit/` | Harvested Reddit images (`{id}.png`) and sidecar metadata (`{id}.json`). |
| `/data/staging/generated/` | OpenRouter synthetic benchmark images and metadata sidecars. |
| `/data/curated/<theme>/` | Curated battlemaps approved into thematic collections (`dungeon`, `wilderness`, `urban`, `building`, `cave`, etc.). |
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
