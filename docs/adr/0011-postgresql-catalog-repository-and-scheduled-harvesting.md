# PostgreSQL catalog repository and scheduled Reddit harvesting

Curator persists domain entity metadata (`Candidate`, `CuratedMap`, `RejectionRecord`, `TrainingRun`, `DatasetSnapshot`, `LoraAdapter`) in PostgreSQL when `DATABASE_URL` is configured, while falling back to `FilesystemCatalogRepository` when absent for offline local development. Scheduled harvesting of `/r/battlemaps` runs via a Kubernetes `CronJob` invoking `POST /api/harvest/reddit` hourly with `sort: hot` and `limit: 25`, persisting run outcomes to a `harvest_runs` ledger table and relying on live catalog entity checks for post deduplication.

## Considered Options

- **In-process Tokio background timer vs. Kubernetes CronJob**: A CronJob decoupled from daemon restarts avoids internal scheduling state, provides visible pod execution logs, and enforces `concurrencyPolicy: Forbid`.
- **Direct volume access vs. HTTP API call in CronJob**: The Hetzner CSI volume is `ReadWriteOnce` and locked to the running Curator pod. A separate CLI container mounting `/data` would fail to start; calling the internal `ClusterIP` HTTP API avoids volume sharing entirely.
- **Monotonic timestamp watermark vs. live catalog deduplication**: Reddit's `hot` ranking algorithm is non-monotonic (posts rise, fall, and linger over days). A monotonic timestamp cursor would prematurely drop newly trending older posts; live deduplication against active `Candidate`, `CuratedMap`, and `RejectionRecord` entities accurately prevents duplicate ingestions while naturally allowing purged items to re-stage if still trending.
- **Fully normalized relational schema vs. hybrid JSONB documents**: Domain entities are stored with relational routing keys (`id`, `created_at`, `source`, `parent_group_id`, `tags TEXT[]`) and a JSONB document payload (`data`), avoiding ORM impedance mismatch with Rust enum/struct models while enabling fast GIN-indexed tag queries.

## Consequences

- Existing filesystem manifests in the cluster (`/data`) must be imported into PostgreSQL via a one-shot migration CLI command and Kubernetes Job (`deploy/curator/job-migrate-catalog.yaml`).
- Curator dependencies include `sqlx` (Postgres async runtime).
- A new database (`curator`) and credentials must be provisioned on the cluster's CloudNativePG `shared-postgres` cluster and injected into `curator-secrets`.
