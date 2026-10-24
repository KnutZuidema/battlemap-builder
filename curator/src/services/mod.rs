pub mod curation;
pub mod description;
pub mod benchmark;
pub mod dataset_export;
pub mod ingestion;
pub mod library;
pub mod reference_provider;
pub mod sections;
pub mod storage_maintenance;
pub mod storage_migration;
pub mod training;
pub use description::*;
pub use sections::{describe_map_sections, SectionDescriptionContext};
pub use curation::*;

pub use benchmark::*;
pub use dataset_export::{DatasetExportService, DeleteDatasetError, FreezeDatasetError};
pub use training::{CreateTrainingRunRequest, TrainingService, TrainingServiceError};
pub use storage_maintenance::StorageMaintenanceService;
pub use storage_migration::{
    discover_disk_blobs, is_migration_completed, run_storage_migration, MigrationOptions,
    MigrationReport, MIGRATION_MARKER_FILE,
};
pub use library::LibraryService;
pub use ingestion::{
    candidate_id_for_post, IngestionError, IngestionOutcome, IngestionProgress, IngestionSummary,
    RedditIngestionService, SkipReason,
};
pub use reference_provider::{
    detect_theme_for_prompt, preprocess_reference_image, preprocess_section_reference_image,
    score_curated_section_for_prompt, select_center_most_section, static_reference_for_theme,
    ReferenceProvenance, ReferenceProvider, ReferenceProviderError, ReferenceResolution,
    StaticReferenceDef, STATIC_REFERENCES,
};
