use std::sync::Arc;
use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{
    normalize_tags, validate_layers_for_curation, CuratedMap, Layer, LayerRole, RejectionReason,
};
use crate::ports::curation_store::CurationStore;
use crate::ports::storage::StorageError;
use crate::services::description::{DescriptionError, DescriptionService};
use crate::services::sections::{describe_map_sections, SectionDescriptionContext};

/// Errors arising during the curation and rejection workflow.
#[derive(Debug, Error)]
pub enum CurationWorkflowError {
    #[error("Candidate validation failed: {0}")]
    ValidationFailed(String),

    #[error("Candidate not found: {0}")]
    NotFound(String),

    #[error("Description generation failed: {0}")]
    DescriptionFailed(#[from] DescriptionError),

    #[error("Storage error: {0}")]
    Storage(String),
}

impl From<StorageError> for CurationWorkflowError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound(msg) => CurationWorkflowError::NotFound(msg),
            StorageError::InvalidData(msg) => CurationWorkflowError::ValidationFailed(msg),
            other => CurationWorkflowError::Storage(other.to_string()),
        }
    }
}

/// Abstract domain workflow port orchestrating candidate curation and rejection.
#[async_trait]
pub trait CurationWorkflow: Send + Sync {
    /// Curates a staged candidate: validates layers, resolves physical description,
    /// atomically commits the candidate, and returns the finalized `CuratedMap` directly in memory.
    async fn curate(
        &self,
        candidate_id: &str,
        tags: Vec<String>,
        layers: Option<Vec<Layer>>,
        description: Option<String>,
    ) -> Result<CuratedMap, CurationWorkflowError>;

    /// Rejects a staged candidate with a structured reason and optional audit notes.
    async fn reject(
        &self,
        candidate_id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), CurationWorkflowError>;
}

/// Domain service orchestrating candidate curation and rejection workflows.
pub struct CurationService {
    store: Arc<dyn CurationStore>,
    description_service: Arc<DescriptionService>,
}

impl CurationService {
    pub fn new(
        store: Arc<dyn CurationStore>,
        description_service: Arc<DescriptionService>,
    ) -> Self {
        Self {
            store,
            description_service,
        }
    }

}

#[async_trait]
impl CurationWorkflow for CurationService {
    async fn curate(
        &self,
        candidate_id: &str,
        tags: Vec<String>,
        layers: Option<Vec<Layer>>,
        description: Option<String>,
    ) -> Result<CuratedMap, CurationWorkflowError> {
        let trimmed_id = candidate_id.trim();
        if trimmed_id.is_empty() {
            return Err(CurationWorkflowError::ValidationFailed(
                "Candidate ID cannot be empty".to_string(),
            ));
        }

        // 1. O(1) candidate lookup
        let candidate = self.store.get_staging_candidate(trimmed_id)?;

        // Validate tags
        let clean_tags = normalize_tags(tags);
        if clean_tags.is_empty() {
            return Err(CurationWorkflowError::ValidationFailed(
                "At least 1 tag/theme is required to curate a candidate".to_string(),
            ));
        }

        // 2. Layer resolution & validation (unassigned/ground rules live in the domain validator)
        let raw_layers = layers.unwrap_or_else(|| candidate.resolved_layers());
        validate_layers_for_curation(&raw_layers)
            .map_err(CurationWorkflowError::ValidationFailed)?;

        let ground_layer = raw_layers
            .iter()
            .find(|l| l.role.is_ground())
            .ok_or_else(|| {
                CurationWorkflowError::ValidationFailed(
                    "Strictly one Ground layer is required to curate a candidate".to_string(),
                )
            })?;

        // 3. 16×16 full-grid curation gate (Ground layer dimensions win over manifest dims)
        let (ground_w, ground_h) = if ground_layer.dimensions != (0, 0) {
            ground_layer.dimensions
        } else {
            candidate.dimensions
        };
        if !candidate.grid.is_curation_eligible(ground_w, ground_h) {
            let (cols, rows) = candidate.grid.lattice_dimensions(ground_w, ground_h);
            return Err(CurationWorkflowError::ValidationFailed(format!(
                "Candidate Ground layer yields fewer than 16×16 full Grid Cells ({cols}×{rows} cells); minimum 16×16 required for curation"
            )));
        }

        // 4. Physical description resolution via DescriptionService (STRICT: no procedural caption fallback)
        let ground_layer_index = ground_layer.index;
        let mut ground_bytes: Option<Vec<u8>> = None;
        let final_description = if let Some(desc) = description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            desc.to_string()
        } else {
            let bytes = self
                .store
                .get_staging_layer_bytes(trimmed_id, ground_layer_index)?;
            let described = self
                .description_service
                .describe(
                    &bytes,
                    Some(ground_layer.filename.as_str()),
                    &clean_tags,
                    candidate.title.as_deref(),
                )
                .await?;
            ground_bytes = Some(bytes);
            described
        };

        // 5. Update ground layer caption with resolved description
        let mut layers_for_commit = raw_layers;
        for layer in &mut layers_for_commit {
            if layer.role == LayerRole::Ground {
                layer.caption = Some(final_description.clone());
            }
        }

        // 6. Derive overlapping 16×16 Map Sections with crop-specific descriptions.
        //    `describe_map_sections` owns the section policy: multi-section maps get a
        //    vision description per Ground-layer crop (with a deterministic per-section
        //    caption fallback); single-section maps reuse the strict map-level physical
        //    description verbatim.
        let ground_bytes = match ground_bytes {
            Some(bytes) => Some(bytes),
            None => self
                .store
                .get_staging_layer_bytes(trimmed_id, ground_layer_index)
                .ok(),
        };
        let ground_image = ground_bytes
            .as_ref()
            .and_then(|bytes| image::load_from_memory(bytes).ok());
        let sections = describe_map_sections(
            &self.description_service,
            SectionDescriptionContext {
                grid: &candidate.grid,
                ground_image: ground_image.as_ref(),
                ground_dimensions: (ground_w, ground_h),
                tags: &clean_tags,
                title: candidate.title.as_deref(),
                map_description: &final_description,
            },
        )
        .await;

        // 7. Construct CuratedMap for atomic commit (bypassing staging manifest disk rewrites)
        let map_to_commit = CuratedMap::with_now(
            trimmed_id,
            Vec::<String>::new(),
            clean_tags,
            candidate.dimensions,
        )
        .with_source(candidate.source)
        .with_grid(candidate.grid)
        .with_layers(layers_for_commit)
        .with_description(final_description)
        .with_sections(sections);

        // 8. Atomic commit, returning the map exactly as written to disk
        let stored_map = self.store.commit_curation(&map_to_commit)?;
        Ok(stored_map)
    }

    async fn reject(
        &self,
        candidate_id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), CurationWorkflowError> {
        let trimmed_id = candidate_id.trim();
        if trimmed_id.is_empty() {
            return Err(CurationWorkflowError::ValidationFailed(
                "Candidate ID cannot be empty".to_string(),
            ));
        }

        self.store.reject_candidate(trimmed_id, reason, notes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use crate::domain::{Candidate, CaptionGenerator, Source};
    use crate::services::description::DescriptionGenerator;

    struct MockDescriptionGenerator {
        result: Mutex<Result<String, DescriptionError>>,
        calls: AtomicUsize,
    }

    impl MockDescriptionGenerator {
        fn success(desc: impl Into<String>) -> Self {
            Self {
                result: Mutex::new(Ok(desc.into())),
                calls: AtomicUsize::new(0),
            }
        }

        fn failure(err: DescriptionError) -> Self {
            Self {
                result: Mutex::new(Err(err)),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl DescriptionGenerator for MockDescriptionGenerator {
        async fn generate_description(
            &self,
            _image_bytes: &[u8],
            _filename: Option<&str>,
            _tags: &[String],
            _title: Option<&str>,
        ) -> Result<String, DescriptionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().unwrap().clone()
        }
    }

    struct MockCurationStore {
        candidate: Mutex<Option<Candidate>>,
        layer_bytes: Mutex<HashMap<usize, Vec<u8>>>,
        committed: Mutex<Option<CuratedMap>>,
        rejected: Mutex<Option<(String, RejectionReason, Option<String>)>>,
        get_staging_candidate_calls: AtomicUsize,
        get_staging_layer_bytes_calls: AtomicUsize,
        commit_curation_calls: AtomicUsize,
        reject_candidate_calls: AtomicUsize,
    }

    impl MockCurationStore {
        fn new(candidate: Option<Candidate>) -> Self {
            Self {
                candidate: Mutex::new(candidate),
                layer_bytes: Mutex::new(HashMap::new()),
                committed: Mutex::new(None),
                rejected: Mutex::new(None),
                get_staging_candidate_calls: AtomicUsize::new(0),
                get_staging_layer_bytes_calls: AtomicUsize::new(0),
                commit_curation_calls: AtomicUsize::new(0),
                reject_candidate_calls: AtomicUsize::new(0),
            }
        }

        fn with_layer_bytes(self, index: usize, bytes: Vec<u8>) -> Self {
            self.layer_bytes.lock().unwrap().insert(index, bytes);
            self
        }
    }

    impl CurationStore for MockCurationStore {
        fn get_staging_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
            self.get_staging_candidate_calls.fetch_add(1, Ordering::SeqCst);
            let guard = self.candidate.lock().unwrap();
            match guard.as_ref() {
                Some(c) if c.id == id => Ok(c.clone()),
                _ => Err(StorageError::NotFound(format!("Candidate '{id}' not found"))),
            }
        }

        fn get_staging_layer_bytes(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError> {
            self.get_staging_layer_bytes_calls.fetch_add(1, Ordering::SeqCst);
            let guard = self.candidate.lock().unwrap();
            if guard.as_ref().map(|c| c.id.as_str()) != Some(id) {
                return Err(StorageError::NotFound(format!("Candidate '{id}' not found")));
            }
            let bytes_guard = self.layer_bytes.lock().unwrap();
            bytes_guard.get(&layer_index).cloned().ok_or_else(|| {
                StorageError::NotFound(format!("Layer {layer_index} not found for candidate '{id}'"))
            })
        }

        fn commit_curation(&self, map: &CuratedMap) -> Result<CuratedMap, StorageError> {
            self.commit_curation_calls.fetch_add(1, Ordering::SeqCst);

            // Mirror the filesystem adapter's persisted form: included layers only,
            // deterministic filenames, and provenance/captions preserved.
            let mut stored = map.clone();
            let mut layers = Vec::new();
            for (curated_idx, src_layer) in map.layers.iter().filter(|l| l.role.is_included()).enumerate() {
                let ext = std::path::Path::new(&src_layer.filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let dest_filename =
                    Layer::format_filename(curated_idx, src_layer.role, src_layer.elevation, ext);
                let mut layer = Layer::new(
                    curated_idx,
                    dest_filename,
                    src_layer.role,
                    src_layer.elevation,
                    if src_layer.dimensions != (0, 0) {
                        src_layer.dimensions
                    } else {
                        map.dimensions
                    },
                );
                if let Some(caption) = &src_layer.caption {
                    layer = layer.with_caption(caption.clone());
                }
                if let Some(source_url) = &src_layer.source_url {
                    layer = layer.with_source_url(source_url.clone());
                }
                layers.push(layer);
            }
            stored.layers = layers;
            *self.committed.lock().unwrap() = Some(stored.clone());
            Ok(stored)
        }

        fn reject_candidate(
            &self,
            id: &str,
            reason: RejectionReason,
            notes: Option<&str>,
        ) -> Result<(), StorageError> {
            self.reject_candidate_calls.fetch_add(1, Ordering::SeqCst);
            let guard = self.candidate.lock().unwrap();
            if guard.as_ref().map(|c| c.id.as_str()) != Some(id) {
                return Err(StorageError::NotFound(format!("Candidate '{id}' not found")));
            }
            *self.rejected.lock().unwrap() = Some((id.to_string(), reason, notes.map(Into::into)));
            Ok(())
        }
    }

    fn sample_candidate() -> Candidate {
        let mut cand = Candidate::with_now("cand_1", Source::Reddit, "0.png", (2048, 2048));
        cand.layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (2048, 2048)),
        ];
        cand
    }

    /// Generator whose output depends on the exact image bytes it receives, so distinct
    /// section crops produce distinct descriptions.
    struct ContentKeyedGenerator {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl DescriptionGenerator for ContentKeyedGenerator {
        async fn generate_description(
            &self,
            image_bytes: &[u8],
            _filename: Option<&str>,
            _tags: &[String],
            _title: Option<&str>,
        ) -> Result<String, DescriptionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let checksum = image_bytes.iter().fold(0u64, |acc, byte| {
                acc.wrapping_mul(1_000_003).wrapping_add(u64::from(*byte))
            });
            Ok(format!(
                "vision crop {checksum:016x} ({} bytes)",
                image_bytes.len()
            ))
        }
    }

    /// Builds a 2048×1024 candidate whose default 64px grid yields exactly two
    /// 16×16 Map Sections (columns 0 and 16). Pixel content varies horizontally so
    /// the two crops are byte-distinct.
    fn two_section_candidate() -> (Candidate, Vec<u8>) {
        let image = image::RgbaImage::from_fn(2048, 1024, |x, y| {
            image::Rgba([
                (x % 251) as u8,
                (y % 241) as u8,
                ((x + y) % 239) as u8,
                255,
            ])
        });
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode gradient PNG");

        let mut cand = Candidate::with_now("cand_two", Source::Reddit, "0.png", (2048, 1024));
        cand.layers = vec![Layer::new(
            0,
            "0.png",
            LayerRole::Ground,
            Some(0),
            (2048, 1024),
        )];
        (cand, png)
    }

    #[tokio::test]
    async fn test_curate_with_user_description_bypasses_vision_and_returns_in_memory() {
        let cand = sample_candidate();
        let store = Arc::new(
            MockCurationStore::new(Some(cand))
                .with_layer_bytes(0, b"\x89PNG\r\n\x1a\nfake_ground_bytes".to_vec()),
        );
        let desc_service = Arc::new(DescriptionService::new(None)); // Unconfigured vision
        let service = CurationService::new(store.clone(), desc_service);

        let result = service
            .curate(
                "cand_1",
                vec!["dungeon".into(), "crypt".into()],
                None,
                Some("A detailed stone dungeon crypt".into()),
            )
            .await
            .expect("curation should succeed with user description");

        // O(1) single candidate lookup; the user description skips map-level vision, but
        // multi-section maps still read Ground bytes once to crop section descriptions.
        assert_eq!(store.get_staging_candidate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.get_staging_layer_bytes_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.commit_curation_calls.load(Ordering::SeqCst), 1);

        // CuratedMap returned directly in memory
        assert_eq!(result.id, "cand_1");
        assert_eq!(result.tags, vec!["dungeon", "crypt"]);
        assert_eq!(result.description.as_deref(), Some("A detailed stone dungeon crypt"));
        assert_eq!(result.layers.len(), 2);
        assert_eq!(result.layers[0].filename, "0_ground.png");
        assert_eq!(result.layers[0].caption.as_deref(), Some("A detailed stone dungeon crypt"));
        assert_eq!(result.layers[1].filename, "1_upper_1.png");
    }

    #[tokio::test]
    async fn test_curate_with_vision_description_resolves_and_populates_captions() {
        let cand = sample_candidate();
        let store = Arc::new(
            MockCurationStore::new(Some(cand))
                .with_layer_bytes(0, b"\x89PNG\r\n\x1a\nfake_ground_bytes".to_vec()),
        );
        let gen = Arc::new(MockDescriptionGenerator::success(
            "An ancient mossy stone dungeon",
        ));
        let desc_service = Arc::new(DescriptionService::new(Some(gen.clone())));
        let service = CurationService::new(store.clone(), desc_service);

        let result = service
            .curate(
                "cand_1",
                vec!["dungeon".into()],
                None,
                None, // Omitted description triggers vision
            )
            .await
            .expect("curation should resolve vision description");

        assert_eq!(store.get_staging_candidate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.get_staging_layer_bytes_calls.load(Ordering::SeqCst), 1);
        assert_eq!(gen.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.commit_curation_calls.load(Ordering::SeqCst), 1);

        assert_eq!(result.description.as_deref(), Some("An ancient mossy stone dungeon"));
        assert_eq!(result.layers[0].caption.as_deref(), Some("An ancient mossy stone dungeon"));
    }

    #[tokio::test]
    async fn test_curate_multi_section_generates_distinct_vision_descriptions() {
        let (cand, ground_png) = two_section_candidate();
        let store = Arc::new(
            MockCurationStore::new(Some(cand)).with_layer_bytes(0, ground_png),
        );
        let generator = Arc::new(ContentKeyedGenerator {
            calls: AtomicUsize::new(0),
        });
        let desc_service = Arc::new(DescriptionService::new(Some(generator.clone())));
        let service = CurationService::new(store.clone(), desc_service);

        let result = service
            .curate("cand_two", vec!["dungeon".into(), "crypt".into()], None, None)
            .await
            .expect("multi-section curation should succeed");

        // Ground bytes are read once and reused for the map description plus both crops.
        assert_eq!(store.get_staging_layer_bytes_calls.load(Ordering::SeqCst), 1);
        // One map-level description plus one per section.
        assert_eq!(generator.calls.load(Ordering::SeqCst), 3);

        assert_eq!(result.sections.len(), 2);
        assert_eq!(
            result
                .sections
                .iter()
                .map(|s| (s.col, s.row))
                .collect::<Vec<_>>(),
            vec![(0, 0), (16, 0)]
        );
        assert!(
            result
                .sections
                .iter()
                .all(|s| s.description.starts_with("vision crop")),
            "Every section must use its crop-specific vision description: {:?}",
            result.sections
        );
        assert_ne!(
            result.sections[0].description, result.sections[1].description,
            "Multi-section maps must never persist identical section descriptions"
        );

        let committed = store
            .committed
            .lock()
            .unwrap()
            .clone()
            .expect("curation must be committed");
        assert_eq!(committed.sections, result.sections);
        assert_eq!(committed.description, result.description);
        assert_eq!(committed.layers[0].caption, result.description);
    }

    #[tokio::test]
    async fn test_curate_multi_section_section_failure_uses_deterministic_fallback() {
        let (cand, ground_png) = two_section_candidate();
        let store = Arc::new(
            MockCurationStore::new(Some(cand)).with_layer_bytes(0, ground_png),
        );
        let generator = Arc::new(MockDescriptionGenerator::failure(
            DescriptionError::GenerationFailed("OpenRouter 500 error".into()),
        ));
        let desc_service = Arc::new(DescriptionService::new(Some(generator.clone())));
        let service = CurationService::new(store.clone(), desc_service);

        // The map-level description is supplied, so only section crops hit the failing generator.
        let result = service
            .curate(
                "cand_two",
                vec!["dungeon".into(), "crypt".into()],
                None,
                Some("Map-level physical description".into()),
            )
            .await
            .expect("section failures must not abort curation");

        assert_eq!(generator.calls.load(Ordering::SeqCst), 2);
        assert_eq!(result.sections.len(), 2);
        let expected =
            CaptionGenerator::generate_with_theme_str("dungeon", &result.tags, None);
        assert!(
            result
                .sections
                .iter()
                .all(|section| section.description == expected),
            "Every failed section must use the deterministic caption fallback: {:?}",
            result.sections
        );

        let committed = store.committed.lock().unwrap().clone().unwrap();
        assert_eq!(committed.sections, result.sections);
    }

    #[tokio::test]
    async fn test_curate_vision_failure_propagates_immediately_without_fallback() {
        let cand = sample_candidate();
        let store = Arc::new(
            MockCurationStore::new(Some(cand))
                .with_layer_bytes(0, b"\x89PNG\r\n\x1a\nfake_ground_bytes".to_vec()),
        );
        let gen = Arc::new(MockDescriptionGenerator::failure(
            DescriptionError::GenerationFailed("OpenRouter 500 error".into()),
        ));
        let desc_service = Arc::new(DescriptionService::new(Some(gen)));
        let service = CurationService::new(store.clone(), desc_service);

        let err = service
            .curate("cand_1", vec!["dungeon".into()], None, None)
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::DescriptionFailed(DescriptionError::GenerationFailed(msg)) => {
                assert!(msg.contains("OpenRouter 500"));
            }
            other => panic!("Expected DescriptionFailed, got {other:?}"),
        }

        // Commit was NEVER called
        assert_eq!(store.commit_curation_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_curate_unconfigured_vision_fails_when_description_omitted() {
        let cand = sample_candidate();
        let store = Arc::new(
            MockCurationStore::new(Some(cand))
                .with_layer_bytes(0, b"\x89PNG\r\n\x1a\nfake_ground_bytes".to_vec()),
        );
        let desc_service = Arc::new(DescriptionService::new(None)); // No generator configured
        let service = CurationService::new(store.clone(), desc_service);

        let err = service
            .curate("cand_1", vec!["dungeon".into()], None, None)
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::DescriptionFailed(DescriptionError::NotConfigured(_)) => {}
            other => panic!("Expected DescriptionFailed(NotConfigured), got {other:?}"),
        }

        assert_eq!(store.commit_curation_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_curate_validation_failure_missing_ground_layer() {
        let cand = sample_candidate();
        let store = Arc::new(MockCurationStore::new(Some(cand)));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        // Custom layers without Ground
        let layers = vec![
            Layer::new(0, "0.png", LayerRole::UpperFloor, Some(1), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::Roof, None, (2048, 2048)),
        ];

        let err = service
            .curate("cand_1", vec!["dungeon".into()], Some(layers), Some("desc".into()))
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::ValidationFailed(msg) => {
                assert!(msg.contains("Strictly one Ground layer is required"), "Got: {msg}");
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
        assert_eq!(store.commit_curation_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_curate_validation_failure_multiple_ground_layers() {
        let cand = sample_candidate();
        let store = Arc::new(MockCurationStore::new(Some(cand)));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        // Custom layers with 2 Ground layers
        let layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::Ground, Some(0), (2048, 2048)),
        ];

        let err = service
            .curate("cand_1", vec!["dungeon".into()], Some(layers), Some("desc".into()))
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::ValidationFailed(msg) => {
                assert!(msg.contains("Strictly one Ground layer is required"), "Got: {msg}");
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
        assert_eq!(store.commit_curation_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_curate_validation_failure_all_excluded() {
        let cand = sample_candidate();
        let store = Arc::new(MockCurationStore::new(Some(cand)));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        let layers = vec![
            Layer::new(0, "0.png", LayerRole::Exclude, None, (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::Exclude, None, (2048, 2048)),
        ];

        let err = service
            .curate("cand_1", vec!["dungeon".into()], Some(layers), Some("desc".into()))
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::ValidationFailed(msg) => {
                assert!(msg.contains("At least 1 layer must be included"), "Got: {msg}");
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_curate_validation_failure_unassigned_layer() {
        let cand = sample_candidate();
        let store = Arc::new(MockCurationStore::new(Some(cand)));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        let layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::Unassigned, None, (2048, 2048)),
        ];

        let err = service
            .curate("cand_1", vec!["dungeon".into()], Some(layers), Some("desc".into()))
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::ValidationFailed(msg) => {
                assert!(msg.contains("unassigned"), "Got: {msg}");
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_curate_validation_failure_invalid_elevation() {
        let cand = sample_candidate();
        let store = Arc::new(MockCurationStore::new(Some(cand)));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        // UpperFloor with negative elevation
        let layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(-1), (2048, 2048)),
        ];

        let err = service
            .curate("cand_1", vec!["dungeon".into()], Some(layers), Some("desc".into()))
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::ValidationFailed(msg) => {
                assert!(msg.contains("Upper floor elevation must be positive"), "Got: {msg}");
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_curate_candidate_not_found() {
        let store = Arc::new(MockCurationStore::new(None));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        let err = service
            .curate("missing_cand", vec!["dungeon".into()], None, Some("desc".into()))
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::NotFound(msg) => {
                assert!(msg.contains("missing_cand"));
            }
            other => panic!("Expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_curate_custom_layers_bypass_manifest_rewrite_and_curates_subset() {
        let cand = sample_candidate(); // 2 layers: 0=Ground, 1=UpperFloor
        let store = Arc::new(
            MockCurationStore::new(Some(cand))
                .with_layer_bytes(0, b"\x89PNG\r\n\x1a\nfake_ground_bytes".to_vec()),
        );
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        // Custom layer assignments: exclude UpperFloor (layer 1)
        let custom_layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::Exclude, None, (2048, 2048)),
        ];

        let result = service
            .curate(
                "cand_1",
                vec!["dungeon".into()],
                Some(custom_layers),
                Some("Ground level only".into()),
            )
            .await
            .expect("curation with excluded layer should succeed");

        assert_eq!(result.layers.len(), 1);
        assert_eq!(result.layers[0].index, 0);
        assert_eq!(result.layers[0].role, LayerRole::Ground);
        assert_eq!(result.layers[0].filename, "0_ground.png");
    }

    #[tokio::test]
    async fn test_reject_delegates_to_store() {
        let cand = sample_candidate();
        let store = Arc::new(MockCurationStore::new(Some(cand)));
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(store.clone(), desc_service);

        service
            .reject("cand_1", RejectionReason::BlurryLowRes, Some("Too blurry"))
            .await
            .expect("rejection should succeed");

        assert_eq!(store.reject_candidate_calls.load(Ordering::SeqCst), 1);
        let rejected = store.rejected.lock().unwrap().clone().unwrap();
        assert_eq!(rejected.0, "cand_1");
        assert_eq!(rejected.1, RejectionReason::BlurryLowRes);
        assert_eq!(rejected.2.as_deref(), Some("Too blurry"));
    }
}
