//! One-shot import of the on-disk catalog layout into the PostgreSQL catalog (ADR-0011).
//!
//! Reads exactly the layout [`crate::adapters::FilesystemCatalogRepository`] writes:
//!
//! | Source | Target table |
//! |---|---|
//! | `staging/{id}/manifest.json` | `candidates` |
//! | `curated/{id}/manifest.json` | `curated_maps` |
//! | `rejected/rejections.json` (plus legacy `rejections.jsonl`) | `rejections` |
//! | `datasets/{id}/snapshot.json` | `dataset_snapshots` |
//! | `training/{id}/run.json` | `training_runs` |
//! | `training/{id}/metrics.jsonl` | `training_metrics` |
//! | `adapters/{id}/adapter.json` | `lora_adapters` |
//!
//! Re-running is idempotent: primary-key tables keep existing rows (`ON CONFLICT DO NOTHING`) and
//! the append-only logs deduplicate identical records. `--force` overwrites primary-key rows
//! instead; `--dry-run` reports the same counts without writing anything.

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapters::postgres_catalog_repository::{
    ImportConflict, PostgresCatalogRepository, RejectionKey, TrainingMetricKey,
};
use crate::domain::{
    BenchmarkMetadata, Candidate, CuratedMap, DatasetSnapshot, LoraAdapter, RejectionRecord,
    TrainingMetricSnapshot, TrainingRun,
};
use crate::ports::storage_error::StorageError;

/// Directory scanned when neither `CURATOR_DATA_DIR` nor `DATA_DIR` is set.
pub const DEFAULT_DATA_DIR: &str = "data";

/// Entity labels used in reports and failure entries.
pub const ENTITY_CANDIDATES: &str = "candidates";
pub const ENTITY_CURATED_MAPS: &str = "curated_maps";
pub const ENTITY_REJECTIONS: &str = "rejections";
pub const ENTITY_DATASET_SNAPSHOTS: &str = "dataset_snapshots";
pub const ENTITY_TRAINING_RUNS: &str = "training_runs";
pub const ENTITY_TRAINING_METRICS: &str = "training_metrics";
pub const ENTITY_LORA_ADAPTERS: &str = "lora_adapters";

/// Configuration options for the disk-to-PostgreSQL catalog migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskToPostgresOptions {
    /// Overwrite rows whose primary key already exists (`--force`) instead of skipping them.
    pub force: bool,
    /// Discover and classify every entity without writing to PostgreSQL (`--dry-run`).
    pub dry_run: bool,
}

impl Default for DiskToPostgresOptions {
    fn default() -> Self {
        Self {
            force: false,
            dry_run: false,
        }
    }
}

/// Per-entity migration tally.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityMigrationCounts {
    /// Entities written (or, for a dry run, that would be written).
    pub migrated: usize,
    /// Entities left untouched because an equivalent row already exists.
    pub skipped: usize,
    /// Entities that could not be read or written.
    pub failed: usize,
}

impl EntityMigrationCounts {
    /// Total entities considered for this entity type.
    pub fn total(&self) -> usize {
        self.migrated + self.skipped + self.failed
    }

    /// Records the outcome of a database import (`true` = row written).
    pub fn record_import(&mut self, written: bool) {
        if written {
            self.migrated += 1;
        } else {
            self.skipped += 1;
        }
    }

    /// Records an entity that is already present and therefore skipped.
    pub fn record_skip(&mut self) {
        self.skipped += 1;
    }

    /// Records a failed read or write.
    pub fn record_failure(&mut self) {
        self.failed += 1;
    }

    /// Adds another tally into this one.
    pub fn merge(&mut self, other: &EntityMigrationCounts) {
        self.migrated += other.migrated;
        self.skipped += other.skipped;
        self.failed += other.failed;
    }
}

/// A single entity that could not be imported, with the path that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationFailure {
    /// Entity label, e.g. [`ENTITY_CANDIDATES`].
    pub entity: String,
    /// Filesystem path the entity was read from.
    pub path: String,
    /// Human-readable reason.
    pub reason: String,
}

impl MigrationFailure {
    /// Builds a failure entry for `entity` read from `path`.
    pub fn new(entity: impl Into<String>, path: impl AsRef<Path>, reason: impl Into<String>) -> Self {
        Self {
            entity: entity.into(),
            path: path.as_ref().display().to_string(),
            reason: reason.into(),
        }
    }
}

/// Structured summary of a disk-to-PostgreSQL catalog migration run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskToPostgresReport {
    pub completed_at: String,
    pub dry_run: bool,
    pub force: bool,
    pub candidates: EntityMigrationCounts,
    pub curated_maps: EntityMigrationCounts,
    pub rejections: EntityMigrationCounts,
    pub dataset_snapshots: EntityMigrationCounts,
    pub training_runs: EntityMigrationCounts,
    pub training_metrics: EntityMigrationCounts,
    pub lora_adapters: EntityMigrationCounts,
    pub failures: Vec<MigrationFailure>,
}

impl DiskToPostgresReport {
    fn new(options: &DiskToPostgresOptions) -> Self {
        Self {
            completed_at: chrono::Utc::now().to_rfc3339(),
            dry_run: options.dry_run,
            force: options.force,
            candidates: EntityMigrationCounts::default(),
            curated_maps: EntityMigrationCounts::default(),
            rejections: EntityMigrationCounts::default(),
            dataset_snapshots: EntityMigrationCounts::default(),
            training_runs: EntityMigrationCounts::default(),
            training_metrics: EntityMigrationCounts::default(),
            lora_adapters: EntityMigrationCounts::default(),
            failures: Vec::new(),
        }
    }

    /// Entity tallies in report order.
    pub fn entity_counts(&self) -> [(&'static str, &EntityMigrationCounts); 7] {
        [
            (ENTITY_CANDIDATES, &self.candidates),
            (ENTITY_CURATED_MAPS, &self.curated_maps),
            (ENTITY_REJECTIONS, &self.rejections),
            (ENTITY_DATASET_SNAPSHOTS, &self.dataset_snapshots),
            (ENTITY_TRAINING_RUNS, &self.training_runs),
            (ENTITY_TRAINING_METRICS, &self.training_metrics),
            (ENTITY_LORA_ADAPTERS, &self.lora_adapters),
        ]
    }

    /// Aggregate tally across every entity type.
    pub fn totals(&self) -> EntityMigrationCounts {
        let mut totals = EntityMigrationCounts::default();
        for (_, counts) in self.entity_counts() {
            totals.merge(counts);
        }
        totals
    }

    /// Whether any entity failed to import.
    pub fn has_failures(&self) -> bool {
        self.totals().failed > 0
    }

    /// Human-readable report for the CLI: per-entity counts plus the failure list.
    pub fn render_summary(&self) -> String {
        let totals = self.totals();
        let status = if self.has_failures() {
            "COMPLETED WITH FAILURES"
        } else {
            "SUCCESS"
        };
        let mut out = String::new();
        let _ = writeln!(out, "==================================================");
        let _ = writeln!(out, "Catalog Migration Report (disk -> PostgreSQL)");
        let _ = writeln!(out, "==================================================");
        let _ = writeln!(out, "Status:             {status}");
        let _ = writeln!(out, "Dry Run:            {}", self.dry_run);
        let _ = writeln!(out, "Force:              {}", self.force);
        let _ = writeln!(out, "Completed At:       {}", self.completed_at);
        let _ = writeln!(out, "--------------------------------------------------");
        let _ = writeln!(out, "{:<20} {:>10} {:>10} {:>10}", "Entity", "Migrated", "Skipped", "Failed");
        for (label, counts) in self.entity_counts() {
            let _ = writeln!(
                out,
                "{:<20} {:>10} {:>10} {:>10}",
                label, counts.migrated, counts.skipped, counts.failed
            );
        }
        let _ = writeln!(out, "--------------------------------------------------");
        let _ = writeln!(
            out,
            "{:<20} {:>10} {:>10} {:>10}",
            "TOTAL", totals.migrated, totals.skipped, totals.failed
        );
        if !self.failures.is_empty() {
            let _ = writeln!(out, "--------------------------------------------------");
            let _ = writeln!(out, "Failures ({}):", self.failures.len());
            for failure in &self.failures {
                let _ = writeln!(
                    out,
                    "  - [{}] {}: {}",
                    failure.entity, failure.path, failure.reason
                );
            }
        }
        let _ = writeln!(out, "==================================================");
        out
    }
}

/// An entity read from disk together with the path it came from.
#[derive(Debug, Clone)]
pub struct Located<T> {
    pub path: PathBuf,
    pub value: T,
}

/// A training metric sample read from `training/{run_id}/metrics.jsonl`.
#[derive(Debug, Clone)]
pub struct LocatedMetric {
    pub path: PathBuf,
    pub run_id: String,
    pub value: TrainingMetricSnapshot,
}

/// Everything discovered under the data directory, ready to import.
#[derive(Debug, Clone, Default)]
pub struct CatalogDiscovery {
    pub candidates: Vec<Located<Candidate>>,
    pub curated_maps: Vec<Located<CuratedMap>>,
    pub rejections: Vec<Located<RejectionRecord>>,
    pub dataset_snapshots: Vec<Located<DatasetSnapshot>>,
    pub training_runs: Vec<Located<TrainingRun>>,
    pub training_metrics: Vec<LocatedMetric>,
    pub lora_adapters: Vec<Located<LoraAdapter>>,
    pub failures: Vec<MigrationFailure>,
}

/// Runs the disk-to-PostgreSQL catalog migration.
///
/// Migrations are expected to have been applied by the caller. `--dry-run` never writes.
pub async fn run_disk_to_postgres_migration(
    data_dir: &Path,
    repository: &PostgresCatalogRepository,
    options: DiskToPostgresOptions,
) -> Result<DiskToPostgresReport, StorageError> {
    tracing::info!(
        data_dir = %data_dir.display(),
        force = options.force,
        dry_run = options.dry_run,
        "Importing filesystem catalog into PostgreSQL..."
    );

    let discovery = discover_catalog_entities(data_dir);
    tracing::info!(
        candidates = discovery.candidates.len(),
        curated_maps = discovery.curated_maps.len(),
        rejections = discovery.rejections.len(),
        dataset_snapshots = discovery.dataset_snapshots.len(),
        training_runs = discovery.training_runs.len(),
        training_metrics = discovery.training_metrics.len(),
        lora_adapters = discovery.lora_adapters.len(),
        discovery_failures = discovery.failures.len(),
        "Discovered catalog entities on disk"
    );

    let index = if options.dry_run {
        Some(repository.import_index().await?)
    } else {
        None
    };
    let conflict = if options.force {
        ImportConflict::Overwrite
    } else {
        ImportConflict::SkipExisting
    };

    let mut report = DiskToPostgresReport::new(&options);
    report.failures = discovery.failures;
    for failure in &report.failures {
        match failure.entity.as_str() {
            ENTITY_CANDIDATES => report.candidates.record_failure(),
            ENTITY_CURATED_MAPS => report.curated_maps.record_failure(),
            ENTITY_REJECTIONS => report.rejections.record_failure(),
            ENTITY_DATASET_SNAPSHOTS => report.dataset_snapshots.record_failure(),
            ENTITY_TRAINING_RUNS => report.training_runs.record_failure(),
            ENTITY_TRAINING_METRICS => report.training_metrics.record_failure(),
            ENTITY_LORA_ADAPTERS => report.lora_adapters.record_failure(),
            _ => {}
        }
    }

    for item in &discovery.candidates {
        match &index {
            Some(index) => {
                if index.candidate_ids.contains(&item.value.id) {
                    report.candidates.record_skip();
                } else {
                    report.candidates.record_import(true);
                }
            }
            None => match repository.import_candidate(&item.value, conflict).await {
                Ok(written) => report.candidates.record_import(written),
                Err(err) => {
                    report.candidates.record_failure();
                    report
                        .failures
                        .push(MigrationFailure::new(ENTITY_CANDIDATES, &item.path, err.to_string()));
                }
            },
        }
    }

    for item in &discovery.curated_maps {
        match &index {
            Some(index) => {
                if index.curated_ids.contains(&item.value.id) {
                    report.curated_maps.record_skip();
                } else {
                    report.curated_maps.record_import(true);
                }
            }
            None => match repository.import_curated(&item.value, conflict).await {
                Ok(written) => report.curated_maps.record_import(written),
                Err(err) => {
                    report.curated_maps.record_failure();
                    report
                        .failures
                        .push(MigrationFailure::new(ENTITY_CURATED_MAPS, &item.path, err.to_string()));
                }
            },
        }
    }

    for item in &discovery.dataset_snapshots {
        match &index {
            Some(index) => {
                if index.dataset_snapshot_ids.contains(&item.value.id) {
                    report.dataset_snapshots.record_skip();
                } else {
                    report.dataset_snapshots.record_import(true);
                }
            }
            None => match repository
                .import_dataset_snapshot(&item.value, conflict)
                .await
            {
                Ok(written) => report.dataset_snapshots.record_import(written),
                Err(err) => {
                    report.dataset_snapshots.record_failure();
                    report.failures.push(MigrationFailure::new(
                        ENTITY_DATASET_SNAPSHOTS,
                        &item.path,
                        err.to_string(),
                    ));
                }
            },
        }
    }

    for item in &discovery.training_runs {
        match &index {
            Some(index) => {
                if index.training_run_ids.contains(&item.value.id) {
                    report.training_runs.record_skip();
                } else {
                    report.training_runs.record_import(true);
                }
            }
            None => match repository.import_training_run(&item.value, conflict).await {
                Ok(written) => report.training_runs.record_import(written),
                Err(err) => {
                    report.training_runs.record_failure();
                    report.failures.push(MigrationFailure::new(
                        ENTITY_TRAINING_RUNS,
                        &item.path,
                        err.to_string(),
                    ));
                }
            },
        }
    }

    for item in &discovery.training_metrics {
        let key = TrainingMetricKey::from_sample(&item.run_id, &item.value);
        match &index {
            Some(index) => {
                if index.training_metrics.contains(&key) {
                    report.training_metrics.record_skip();
                } else {
                    report.training_metrics.record_import(true);
                }
            }
            None => match repository
                .import_training_metric(&item.run_id, &item.value)
                .await
            {
                Ok(written) => report.training_metrics.record_import(written),
                Err(err) => {
                    report.training_metrics.record_failure();
                    report.failures.push(MigrationFailure::new(
                        ENTITY_TRAINING_METRICS,
                        &item.path,
                        err.to_string(),
                    ));
                }
            },
        }
    }

    for item in &discovery.lora_adapters {
        match &index {
            Some(index) => {
                if index.lora_adapter_ids.contains(&item.value.id) {
                    report.lora_adapters.record_skip();
                } else {
                    report.lora_adapters.record_import(true);
                }
            }
            None => match repository.import_lora_adapter(&item.value, conflict).await {
                Ok(written) => report.lora_adapters.record_import(written),
                Err(err) => {
                    report.lora_adapters.record_failure();
                    report.failures.push(MigrationFailure::new(
                        ENTITY_LORA_ADAPTERS,
                        &item.path,
                        err.to_string(),
                    ));
                }
            },
        }
    }

    for item in &discovery.rejections {
        match &index {
            Some(index) => {
                let key = RejectionKey::from_record(&item.value);
                if index.rejections.contains(&key) {
                    report.rejections.record_skip();
                } else {
                    report.rejections.record_import(true);
                }
            }
            None => match repository.import_rejection(&item.value).await {
                Ok(written) => report.rejections.record_import(written),
                Err(err) => {
                    report.rejections.record_failure();
                    report.failures.push(MigrationFailure::new(
                        ENTITY_REJECTIONS,
                        &item.path,
                        err.to_string(),
                    ));
                }
            },
        }
    }

    let totals = report.totals();
    tracing::info!(
        migrated = totals.migrated,
        skipped = totals.skipped,
        failed = totals.failed,
        dry_run = report.dry_run,
        "Catalog migration completed"
    );

    Ok(report)
}

/// Reads every catalog entity under `data_dir` without touching the database.
///
/// Unreadable or malformed files are recorded in [`CatalogDiscovery::failures`] and skipped; a
/// missing directory simply yields no entities.
pub fn discover_catalog_entities(data_dir: &Path) -> CatalogDiscovery {
    let mut discovery = CatalogDiscovery::default();

    discovery.candidates = scan_directory(
        &data_dir.join("staging"),
        "manifest.json",
        ENTITY_CANDIDATES,
        &mut discovery.failures,
        parse_candidate_manifest,
    );
    discovery.curated_maps = scan_directory(
        &data_dir.join("curated"),
        "manifest.json",
        ENTITY_CURATED_MAPS,
        &mut discovery.failures,
        |path| read_json::<CuratedMap>(path),
    );
    discovery.dataset_snapshots = scan_directory(
        &data_dir.join("datasets"),
        "snapshot.json",
        ENTITY_DATASET_SNAPSHOTS,
        &mut discovery.failures,
        |path| read_json::<DatasetSnapshot>(path),
    );
    discovery.training_runs = scan_directory(
        &data_dir.join("training"),
        "run.json",
        ENTITY_TRAINING_RUNS,
        &mut discovery.failures,
        |path| read_json::<TrainingRun>(path),
    );
    discovery.lora_adapters = scan_directory(
        &data_dir.join("adapters"),
        "adapter.json",
        ENTITY_LORA_ADAPTERS,
        &mut discovery.failures,
        |path| read_json::<LoraAdapter>(path),
    );
    discovery.training_metrics =
        discover_training_metrics(&data_dir.join("training"), &mut discovery.failures);
    discovery.rejections = discover_rejections(data_dir, &mut discovery.failures);

    discovery
}

/// Scans `dir/*/{file_name}`, parsing each file with `parse`.
fn scan_directory<T, F>(
    dir: &Path,
    file_name: &str,
    entity: &str,
    failures: &mut Vec<MigrationFailure>,
    mut parse: F,
) -> Vec<Located<T>>
where
    F: FnMut(&Path) -> Result<T, StorageError>,
{
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            failures.push(MigrationFailure::new(entity, dir, err.to_string()));
            return Vec::new();
        }
    };

    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect();
    subdirs.sort();

    let mut items = Vec::new();
    for subdir in subdirs {
        let path = subdir.join(file_name);
        if !path.is_file() {
            continue;
        }
        match parse(&path) {
            Ok(value) => items.push(Located { path, value }),
            Err(err) => failures.push(MigrationFailure::new(entity, &path, err.to_string())),
        }
    }
    items
}

/// Parses a staging manifest, falling back to the pre-#176 [`BenchmarkMetadata`] shape.
fn parse_candidate_manifest(path: &Path) -> Result<Candidate, StorageError> {
    let bytes = std::fs::read(path)?;
    match serde_json::from_slice::<Candidate>(&bytes) {
        Ok(candidate) => Ok(candidate),
        Err(candidate_err) => serde_json::from_slice::<BenchmarkMetadata>(&bytes)
            .map(|metadata| metadata.to_candidate())
            .map_err(|_| {
                StorageError::InvalidData(format!(
                    "manifest is neither a Candidate nor a legacy BenchmarkMetadata document: {candidate_err}"
                ))
            }),
    }
}

/// Reads the rejection log in append order.
///
/// The canonical location is `rejected/rejections.json` (a JSON array, as written by
/// [`crate::adapters::FilesystemCatalogRepository`]); the legacy `rejections.jsonl` root file is
/// imported too when present.
fn discover_rejections(data_dir: &Path, failures: &mut Vec<MigrationFailure>) -> Vec<Located<RejectionRecord>> {
    let mut records = Vec::new();

    let canonical = data_dir.join("rejected").join("rejections.json");
    if canonical.is_file() {
        match read_json::<Vec<RejectionRecord>>(&canonical) {
            Ok(items) => records.extend(items.into_iter().map(|value| Located {
                path: canonical.clone(),
                value,
            })),
            Err(err) => failures.push(MigrationFailure::new(
                ENTITY_REJECTIONS,
                &canonical,
                err.to_string(),
            )),
        }
    }

    let legacy = data_dir.join("rejections.jsonl");
    if legacy.is_file() {
        match std::fs::read_to_string(&legacy) {
            Ok(content) => {
                for (index, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<RejectionRecord>(trimmed) {
                        Ok(value) => records.push(Located {
                            path: legacy.clone(),
                            value,
                        }),
                        Err(err) => failures.push(MigrationFailure::new(
                            ENTITY_REJECTIONS,
                            &legacy,
                            format!("line {}: {err}", index + 1),
                        )),
                    }
                }
            }
            Err(err) => failures.push(MigrationFailure::new(
                ENTITY_REJECTIONS,
                &legacy,
                err.to_string(),
            )),
        }
    }

    records
}

/// Reads every `training/{run_id}/metrics.jsonl` sample in file order.
fn discover_training_metrics(
    training_dir: &Path,
    failures: &mut Vec<MigrationFailure>,
) -> Vec<LocatedMetric> {
    let entries = match std::fs::read_dir(training_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            failures.push(MigrationFailure::new(
                ENTITY_TRAINING_METRICS,
                training_dir,
                err.to_string(),
            ));
            return Vec::new();
        }
    };

    let mut run_dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect();
    run_dirs.sort();

    let mut metrics = Vec::new();
    for run_dir in run_dirs {
        let path = run_dir.join("metrics.jsonl");
        if !path.is_file() {
            continue;
        }
        let Some(run_id) = run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            failures.push(MigrationFailure::new(
                ENTITY_TRAINING_METRICS,
                &path,
                "training run directory name is not valid UTF-8",
            ));
            continue;
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                failures.push(MigrationFailure::new(
                    ENTITY_TRAINING_METRICS,
                    &path,
                    err.to_string(),
                ));
                continue;
            }
        };
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<TrainingMetricSnapshot>(trimmed) {
                Ok(value) => metrics.push(LocatedMetric {
                    path: path.clone(),
                    run_id: run_id.clone(),
                    value,
                }),
                Err(err) => failures.push(MigrationFailure::new(
                    ENTITY_TRAINING_METRICS,
                    &path,
                    format!("line {}: {err}", index + 1),
                )),
            }
        }
    }
    metrics
}

/// Reads and deserializes a JSON document.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| StorageError::InvalidData(format!("invalid JSON: {err}")))
}
