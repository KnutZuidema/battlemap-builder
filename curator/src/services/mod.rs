pub mod description;
pub mod benchmark;
pub mod ingestion;
pub mod reference_provider;
pub use description::*;

pub use benchmark::*;
pub use ingestion::{
    candidate_id_for_post, IngestionError, IngestionOutcome, IngestionProgress, IngestionSummary,
    RedditIngestionService, SkipReason,
};
pub use reference_provider::{
    detect_theme_for_prompt, preprocess_reference_image, static_reference_for_theme,
    ReferenceProvenance, ReferenceProvider, ReferenceProviderError, ReferenceResolution,
    StaticReferenceDef, STATIC_REFERENCES,
};
