pub mod ingestion;

pub use ingestion::{
    candidate_id_for_post, IngestionError, IngestionOutcome, IngestionSummary,
    RedditIngestionService, SkipReason,
};
