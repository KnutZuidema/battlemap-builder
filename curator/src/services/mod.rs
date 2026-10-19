pub mod benchmark;
pub mod ingestion;
pub mod reference_provider;

pub use benchmark::*;
pub use ingestion::{
    candidate_id_for_post, IngestionError, IngestionOutcome, IngestionProgress, IngestionSummary,
    RedditIngestionService, SkipReason,
};
pub use reference_provider::{
    crop_section_image, detect_theme_for_prompt, preprocess_reference_image,
    preprocess_section_reference_image, score_curated_map_for_prompt,
    score_curated_section_for_prompt, select_center_most_section, static_reference_for_theme,
    ReferenceProvenance, ReferenceProvider, ReferenceProviderError, ReferenceResolution,
    StaticReferenceDef, STATIC_REFERENCES,
};
