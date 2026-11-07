//! Filesystem-backed persistent cost ledger adapter.
//!
//! Stores generation telemetry, all-time spend, and per-model metrics in a persistent
//! JSON ledger under the resolved data directory (`<data dir>/telemetry/cost_ledger.json`) while
//! maintaining in-memory session metrics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::ports::cost_tracker::{
    CostRecord, CostStats, CostTracker, CostTrackerError, GenerationCostRecord, ModelCostStats,
    TrainingCostRecord,
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// On-disk representation of the persistent cost ledger (`<data dir>/telemetry/cost_ledger.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PersistentLedger {
    /// Cumulative dollar spend across all sessions for image generation.
    pub total_spend_usd: f64,
    /// Total number of images generated across all sessions.
    pub total_generations: u64,
    /// Cumulative dollar spend across all sessions for training runs (ADR-0009).
    #[serde(default)]
    pub training_spend_usd: f64,
    /// Total number of training runs recorded (ADR-0009).
    #[serde(default)]
    pub training_runs: u64,
    /// Breakdown of spending, image counts, and tokens keyed by model ID.
    pub by_model: HashMap<String, ModelCostStats>,
    /// Chronological history of every image generation run and training run.
    pub history: Vec<CostRecord>,
}
/// Filesystem-backed adapter implementing `CostTracker`.
///
/// Features:
/// - Thread-safe synchronization via `tokio::sync::RwLock`.
/// - Atomic file writes using temporary files and renames to prevent corruption.
/// - In-memory session tracking that can be reset independently of persistent history.
#[derive(Debug, Clone)]
pub struct FileCostLedger {
    path: PathBuf,
    tracker: InMemoryCostTracker,
}

impl FileCostLedger {
    /// Creates a new `FileCostLedger` anchored at the provided path.
    /// If an existing ledger file is found, it will be loaded; otherwise, a fresh ledger starts.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let persistent = Self::load_from_disk(&path);

        Self {
            path,
            tracker: InMemoryCostTracker::with_persistent_ledger(persistent),
        }
    }

    /// Path to the on-disk ledger file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and deserializes the persistent ledger from disk if present.
    /// Automatically migrates legacy untagged `GenerationCostRecord` entries to the tagged `CostRecord` union (ADR-0009).
    fn load_from_disk(path: &Path) -> PersistentLedger {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(val) => {
                        let needs_migration = val
                            .get("history")
                            .and_then(|h| h.as_array())
                            .map(|arr| {
                                arr.iter().any(|item| {
                                    item.as_object().is_some_and(|obj| {
                                        !obj.contains_key("type") && !obj.contains_key("kind")
                                    })
                                })
                            })
                            .unwrap_or(false);

                        match serde_json::from_value::<PersistentLedger>(val) {
                            Ok(ledger) => {
                                if needs_migration {
                                    tracing::info!(
                                        path = %path.display(),
                                        "Migrating legacy cost ledger entries to tagged CostRecord union and persisting"
                                    );
                                    let _ = Self::save_ledger_to_path(path, &ledger);
                                }
                                ledger
                            }
                            Err(err) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %err,
                                    "Existing cost ledger is corrupted; starting with fresh ledger"
                                );
                                PersistentLedger::default()
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %err,
                            "Failed to parse existing cost ledger as JSON; starting with fresh ledger"
                        );
                        PersistentLedger::default()
                    }
                },
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "Failed to read existing cost ledger; starting with fresh ledger"
                    );
                    PersistentLedger::default()
                }
            }
        } else {
            PersistentLedger::default()
        }
    }

    fn save_ledger_to_path(path: &Path, ledger: &PersistentLedger) -> Result<(), CostTrackerError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;

        let temp_filename = format!(
            ".cost_ledger_{}_{}.tmp",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let temp_path = parent.join(temp_filename);

        let json_bytes = serde_json::to_vec_pretty(ledger)?;
        std::fs::write(&temp_path, &json_bytes)?;

        if let Err(e) = std::fs::rename(&temp_path, path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(CostTrackerError::Io(e));
        }

        Ok(())
    }
    /// Atomically persists the ledger to disk by writing to a temporary file
    /// in the same directory and renaming it over the destination path.
    fn atomic_save(&self, ledger: &PersistentLedger) -> Result<(), CostTrackerError> {
        Self::save_ledger_to_path(&self.path, ledger)
    }
}

/// Mutable ledger state shared by every [`CostTracker`] implementation: the persistent
/// (serializable) ledger plus per-session counters.
///
/// Adapters only add synchronization and persistence around this state, so the accounting rules
/// live in exactly one place.
#[derive(Debug, Clone, Default)]
struct LedgerState {
    persistent: PersistentLedger,
    session_spend_usd: f64,
    session_generations: u64,
}

impl LedgerState {
    fn record_generation(&mut self, record: GenerationCostRecord) {
        self.persistent.total_spend_usd += record.cost_usd;
        self.persistent.total_generations += 1;

        let model_stats = self
            .persistent
            .by_model
            .entry(record.model.clone())
            .or_default();
        model_stats.spend_usd += record.cost_usd;
        model_stats.generation_count += 1;
        model_stats.total_tokens += record.prompt_tokens.unwrap_or(0) as u64
            + record.completion_tokens.unwrap_or(0) as u64;

        self.persistent
            .history
            .push(CostRecord::Generation(record.clone()));

        self.session_spend_usd += record.cost_usd;
        self.session_generations += 1;
    }

    fn record_training_cost(&mut self, record: TrainingCostRecord) {
        self.persistent.training_spend_usd += record.cost_usd;
        self.persistent.training_runs += 1;
        self.persistent
            .history
            .push(CostRecord::Training(record.clone()));

        self.session_spend_usd += record.cost_usd;
    }

    fn stats(&self) -> CostStats {
        let generation_spend = self.persistent.total_spend_usd;
        let training_spend = self.persistent.training_spend_usd;
        let all_time_spend = generation_spend + training_spend;

        CostStats {
            session_spend_usd: self.session_spend_usd,
            total_spend_usd: all_time_spend,
            total_generations: self.persistent.total_generations,
            session_generations: self.session_generations,
            by_model: self.persistent.by_model.clone(),
            history: self.persistent.history.clone(),
            all_time_spend,
            session_spend: self.session_spend_usd,
            training_spend,
            generation_spend,
            training_runs: self.persistent.training_runs,
        }
    }

    fn reset_session(&mut self) {
        self.session_spend_usd = 0.0;
        self.session_generations = 0;
    }
}

/// In-process [`CostTracker`] that keeps cost telemetry entirely in memory.
///
/// Used by application states that must not read or write the filesystem, such as test
/// fixtures and the recording-free `AppState` constructors. Production uses [`FileCostLedger`]
/// anchored at the resolved data directory.
#[derive(Debug, Clone, Default)]
pub struct InMemoryCostTracker {
    state: Arc<RwLock<LedgerState>>,
}

impl InMemoryCostTracker {
    fn with_persistent_ledger(persistent: PersistentLedger) -> Self {
        Self {
            state: Arc::new(RwLock::new(LedgerState {
                persistent,
                ..LedgerState::default()
            })),
        }
    }
}

#[async_trait]
impl CostTracker for InMemoryCostTracker {
    async fn record_generation(
        &self,
        record: GenerationCostRecord,
    ) -> Result<(), CostTrackerError> {
        self.state.write().await.record_generation(record);
        Ok(())
    }

    async fn record_training_cost(
        &self,
        record: TrainingCostRecord,
    ) -> Result<(), CostTrackerError> {
        self.state.write().await.record_training_cost(record);
        Ok(())
    }

    async fn get_stats(&self) -> Result<CostStats, CostTrackerError> {
        Ok(self.state.read().await.stats())
    }

    async fn reset_session(&self) -> Result<(), CostTrackerError> {
        self.state.write().await.reset_session();
        Ok(())
    }
}

#[async_trait]
impl CostTracker for FileCostLedger {
    async fn record_generation(
        &self,
        record: GenerationCostRecord,
    ) -> Result<(), CostTrackerError> {
        let mut state = self.tracker.state.write().await;
        state.record_generation(record);
        self.atomic_save(&state.persistent)
    }

    async fn record_training_cost(
        &self,
        record: TrainingCostRecord,
    ) -> Result<(), CostTrackerError> {
        let mut state = self.tracker.state.write().await;
        state.record_training_cost(record);
        self.atomic_save(&state.persistent)
    }

    async fn get_stats(&self) -> Result<CostStats, CostTrackerError> {
        Ok(self.tracker.state.read().await.stats())
    }

    async fn reset_session(&self) -> Result<(), CostTrackerError> {
        self.tracker.state.write().await.reset_session();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_empty_ledger_stats() {
        let temp_dir = TempDir::new().unwrap();
        let ledger_path = temp_dir.path().join("telemetry").join("cost_ledger.json");
        let ledger = FileCostLedger::new(&ledger_path);

        let stats = ledger.get_stats().await.expect("stats succeeds");
        assert_eq!(stats.session_spend_usd, 0.0);
        assert_eq!(stats.total_spend_usd, 0.0);
        assert_eq!(stats.total_generations, 0);
        assert_eq!(stats.session_generations, 0);
        assert!(stats.by_model.is_empty());
        assert!(stats.history.is_empty());
    }

    #[tokio::test]
    async fn test_record_generation_and_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let ledger_path = temp_dir.path().join("telemetry").join("cost_ledger.json");
        let ledger = FileCostLedger::new(&ledger_path);

        let record1 = GenerationCostRecord::new(
            "bench_1",
            "google/gemini-3.1-flash-image",
            "Stone Crypt",
            0.015,
            Some(120),
            Some(480),
            350,
        );
        ledger
            .record_generation(record1)
            .await
            .expect("recording succeeds");

        assert!(ledger_path.is_file(), "Ledger JSON file must exist on disk");

        let stats = ledger.get_stats().await.expect("stats succeeds");
        assert_eq!(stats.total_generations, 1);
        assert_eq!(stats.session_generations, 1);
        assert!((stats.total_spend_usd - 0.015).abs() < 1e-6);
        assert!((stats.session_spend_usd - 0.015).abs() < 1e-6);
        assert_eq!(stats.history.len(), 1);
        assert_eq!(stats.history[0].as_generation().unwrap().id, "bench_1");

        let model_stats = stats.by_model.get("google/gemini-3.1-flash-image").unwrap();
        assert_eq!(model_stats.generation_count, 1);
        assert!((model_stats.spend_usd - 0.015).abs() < 1e-6);
        assert_eq!(model_stats.total_tokens, 600);

        // Verify JSON contents directly on disk
        let disk_content = std::fs::read_to_string(&ledger_path).unwrap();
        let disk_ledger: PersistentLedger = serde_json::from_str(&disk_content).unwrap();
        assert_eq!(disk_ledger.total_generations, 1);
        assert_eq!(disk_ledger.history.len(), 1);
        assert_eq!(disk_ledger.history[0].as_generation().unwrap().prompt_name, "Stone Crypt");

        // Record a second generation with another model
        let record2 = GenerationCostRecord::new(
            "bench_2",
            "meta/muse-image",
            "Underdark Chasm",
            0.025,
            Some(200),
            Some(800),
            600,
        );
        ledger
            .record_generation(record2)
            .await
            .expect("recording succeeds");

        let stats2 = ledger.get_stats().await.unwrap();
        assert_eq!(stats2.total_generations, 2);
        assert_eq!(stats2.session_generations, 2);
        assert!((stats2.total_spend_usd - 0.040).abs() < 1e-6);
        assert_eq!(stats2.by_model.len(), 2);

        // Verify persistence across new FileCostLedger instance
        let ledger_reloaded = FileCostLedger::new(&ledger_path);
        let reloaded_stats = ledger_reloaded.get_stats().await.unwrap();
        assert_eq!(reloaded_stats.total_generations, 2);
        assert!((reloaded_stats.total_spend_usd - 0.040).abs() < 1e-6);
        // Brand new instance starts with clean session
        assert_eq!(reloaded_stats.session_generations, 0);
        assert_eq!(reloaded_stats.session_spend_usd, 0.0);
    }

    #[tokio::test]
    async fn test_session_reset() {
        let temp_dir = TempDir::new().unwrap();
        let ledger_path = temp_dir.path().join("telemetry").join("cost_ledger.json");
        let ledger = FileCostLedger::new(&ledger_path);

        let record = GenerationCostRecord::new(
            "bench_1",
            "google/gemini-3.1-flash-image",
            "Stone Crypt",
            0.015,
            Some(100),
            Some(200),
            300,
        );
        ledger.record_generation(record).await.unwrap();

        let stats_before = ledger.get_stats().await.unwrap();
        assert_eq!(stats_before.session_generations, 1);
        assert_eq!(stats_before.total_generations, 1);
        assert!((stats_before.session_spend_usd - 0.015).abs() < 1e-6);

        // Reset session
        ledger.reset_session().await.unwrap();

        let stats_after = ledger.get_stats().await.unwrap();
        assert_eq!(stats_after.session_generations, 0);
        assert_eq!(stats_after.session_spend_usd, 0.0);
        // Persistent metrics remain completely intact
        assert_eq!(stats_after.total_generations, 1);
        assert!((stats_after.total_spend_usd - 0.015).abs() < 1e-6);
        assert_eq!(stats_after.history.len(), 1);
    }
    #[tokio::test]
    async fn test_training_cost_recording_and_legacy_migration() {
        let temp_dir = TempDir::new().unwrap();
        let ledger_path = temp_dir.path().join("telemetry").join("cost_ledger.json");

        // Write a legacy cost ledger file with untagged history entries
        let legacy_json = serde_json::json!({
            "total_spend_usd": 0.030,
            "total_generations": 2,
            "by_model": {},
            "history": [
                {
                    "id": "gen_legacy_1",
                    "model": "google/gemini-3.1-flash-image",
                    "prompt_name": "Crypt",
                    "cost_usd": 0.015,
                    "latency_ms": 300,
                    "timestamp": "2026-10-24T10:00:00Z"
                },
                {
                    "id": "gen_legacy_2",
                    "model": "meta/muse-image",
                    "prompt_name": "Tavern",
                    "cost_usd": 0.015,
                    "latency_ms": 400,
                    "timestamp": "2026-10-24T10:05:00Z"
                }
            ]
        });
        std::fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
        std::fs::write(&ledger_path, serde_json::to_string_pretty(&legacy_json).unwrap()).unwrap();

        // Loading FileCostLedger should automatically migrate the file
        let ledger = FileCostLedger::new(&ledger_path);
        let stats = ledger.get_stats().await.unwrap();
        assert_eq!(stats.history.len(), 2);
        assert_eq!(stats.total_generations, 2);
        assert_eq!(stats.generation_spend, 0.030);
        assert_eq!(stats.training_spend, 0.0);
        assert_eq!(stats.all_time_spend, 0.030);
        assert_eq!(stats.training_runs, 0);

        // Verify disk content is now tagged
        let on_disk_str = std::fs::read_to_string(&ledger_path).unwrap();
        let disk_val: serde_json::Value = serde_json::from_str(&on_disk_str).unwrap();
        let first_entry = &disk_val["history"][0];
        assert_eq!(first_entry["type"], "generation");
        assert_eq!(first_entry["id"], "gen_legacy_1");

        // Record a training cost
        let training_rec = TrainingCostRecord::new(
            "run-001",
            "calibration",
            1500,
            1200.0,
            1.95,
            "succeeded",
        );
        ledger.record_training_cost(training_rec).await.unwrap();

        let stats_after_train = ledger.get_stats().await.unwrap();
        assert_eq!(stats_after_train.history.len(), 3);
        assert_eq!(stats_after_train.training_runs, 1);
        assert!((stats_after_train.training_spend - 1.95).abs() < 1e-6);
        assert!((stats_after_train.all_time_spend - 1.98).abs() < 1e-6);
        assert!((stats_after_train.session_spend - 1.95).abs() < 1e-6);

        // Verify the newly written disk content has tagged training entry
        let on_disk_str2 = std::fs::read_to_string(&ledger_path).unwrap();
        let disk_val2: serde_json::Value = serde_json::from_str(&on_disk_str2).unwrap();
        let train_entry = &disk_val2["history"][2];
        assert_eq!(train_entry["type"], "training");
        assert_eq!(train_entry["run_id"], "run-001");
        assert_eq!(train_entry["policy"], "calibration");
    }
}
