use std::sync::Arc;
use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{
    normalize_tags, validate_layers_for_curation, CuratedMap, Layer, RejectionReason,
};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::storage_error::StorageError;
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
///
/// Curation is a saga over the two decoupled ports: the curated layer blobs are written first,
/// the curated manifest is then committed to the catalog, and only a successful commit purges the
/// staging bundle. A failed commit rolls the freshly written curated blobs back, so a retry always
/// starts from the untouched staging candidate.
///
/// No `<id>_lora1024.{png,txt}` companion is normalized or written (ADR-0007): the LoRA training
/// samples are derived per Map Section on demand by the dataset export.
pub struct CurationService {
    catalog: Arc<dyn CatalogRepository + Send + Sync>,
    blobs: Arc<dyn BlobStorage + Send + Sync>,
    description_service: Arc<DescriptionService>,
}

impl CurationService {
    pub fn new(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
        description_service: Arc<DescriptionService>,
    ) -> Self {
        Self {
            catalog,
            blobs,
            description_service,
        }
    }

    /// Canonical staging address of layer `index` of candidate `id`.
    fn staging_key(id: &str, index: usize) -> BlobKey {
        BlobKey::StagingLayer {
            id: id.to_string(),
            index,
        }
    }

    /// Best-effort rollback of the curated blobs written by the current curation call.
    async fn rollback_curated(&self, written: &[BlobKey]) {
        for key in written {
            if let Err(err) = self.blobs.delete_blob(key).await {
                tracing::warn!(
                    path = %key.to_path(),
                    error = %err,
                    "Failed to roll back a curated blob after a failed curation commit"
                );
            }
        }
    }

    /// Best-effort purge of a candidate's staging bundle: every staging blob of the candidate plus
    /// its staging manifest. Cleanup failures are logged and never fail the workflow.
    async fn purge_staging(&self, id: &str) {
        let staging_dir = BlobKey::StagingDir {
            id: Some(id.to_string()),
        };
        match self.blobs.list_prefix(&staging_dir).await {
            Ok(keys) => {
                for key in keys {
                    if let Err(err) = self.blobs.delete_blob(&key).await {
                        tracing::warn!(
                            path = %key.to_path(),
                            error = %err,
                            "Failed to delete a staging blob during curation cleanup"
                        );
                    }
                }
            }
            Err(err) => tracing::warn!(
                id = %id,
                error = %err,
                "Failed to list the staging blobs of a curated candidate"
            ),
        }

        if let Err(err) = self.catalog.delete_candidate(id).await {
            tracing::warn!(
                id = %id,
                error = %err,
                "Failed to delete the curated candidate from staging"
            );
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
        let id = candidate_id.trim();
        if id.is_empty() {
            return Err(CurationWorkflowError::ValidationFailed(
                "Candidate ID cannot be empty".to_string(),
            ));
        }

        // 1. O(1) candidate lookup
        let candidate = self.catalog.get_candidate(id).await?;

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
            })?
            .clone();

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

        // 4. Read the bytes of every INCLUDED layer exactly once: the Ground bytes feed the
        //    physical description and the section crops, and every included layer is needed to
        //    write its curated blob.
        let mut included: Vec<(Layer, Vec<u8>)> = Vec::new();
        for layer in raw_layers.iter().filter(|l| l.role.is_included()) {
            let bytes = self
                .blobs
                .read_blob(&Self::staging_key(id, layer.index))
                .await?;
            included.push((layer.clone(), bytes));
        }
        let ground_position = included
            .iter()
            .position(|(layer, _)| layer.role.is_ground())
            .ok_or_else(|| {
                CurationWorkflowError::ValidationFailed(
                    "Strictly one Ground layer is required to curate a candidate".to_string(),
                )
            })?;
        let ground_bytes = included[ground_position].1.as_slice();

        // 5. Physical description resolution via DescriptionService (STRICT: no procedural caption
        //    fallback). A user-supplied description bypasses vision entirely.
        let final_description = if let Some(desc) = description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            desc.to_string()
        } else {
            self.description_service
                .describe(
                    ground_bytes,
                    Some(ground_layer.filename.as_str()),
                    &clean_tags,
                    candidate.title.as_deref(),
                )
                .await?
        };

        // 6. Derive overlapping 16×16 Map Sections with crop-specific descriptions from the Ground
        //    bytes already in hand (never re-read). `describe_map_sections` owns the section policy:
        //    multi-section maps get a vision description per Ground-layer crop (reusing the
        //    map-level physical description per section on failure); single-section maps reuse the
        //    strict map-level physical description verbatim.
        let ground_image = image::load_from_memory(ground_bytes).ok();
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

        // 7. Build the curated layers (deterministic filenames counting only INCLUDED layers) and
        //    write each CuratedLayer blob, remembering every key written by THIS call so a later
        //    failure can roll exactly those back.
        let mut curated_layers: Vec<Layer> = Vec::with_capacity(included.len());
        let mut written: Vec<BlobKey> = Vec::with_capacity(included.len());
        for (curated_index, (layer, bytes)) in included.into_iter().enumerate() {
            let mut curated_layer = Layer::new(
                curated_index,
                Layer::format_filename(curated_index, layer.role, layer.elevation, "png"),
                layer.role,
                layer.elevation,
                if layer.dimensions != (0, 0) {
                    layer.dimensions
                } else {
                    candidate.dimensions
                },
            );
            let caption = if layer.role.is_ground() {
                Some(final_description.clone())
            } else {
                layer.caption.clone()
            };
            if let Some(caption) = caption {
                curated_layer = curated_layer.with_caption(caption);
            }
            if let Some(source_url) = &layer.source_url {
                curated_layer = curated_layer.with_source_url(source_url.clone());
            }

            let key = BlobKey::CuratedLayer {
                id: id.to_string(),
                index: curated_index,
                role: layer.role,
                elevation: layer.elevation,
            };
            if let Err(err) = self.blobs.write_blob(&key, &bytes).await {
                self.rollback_curated(&written).await;
                return Err(err.into());
            }
            written.push(key);
            curated_layers.push(curated_layer);
        }

        let map = CuratedMap::with_now(
            id,
            Vec::<String>::new(),
            clean_tags,
            candidate.dimensions,
        )
        .with_source(candidate.source)
        .with_grid(candidate.grid)
        .with_layers(curated_layers)
        .with_description(final_description)
        .with_sections(sections);

        // 8. Commit the manifest; a failure rolls the freshly written curated blobs back and leaves
        //    the staging candidate untouched so curation can be retried.
        if let Err(err) = self.catalog.save_curated(&map).await {
            self.rollback_curated(&written).await;
            return Err(err.into());
        }

        // 9. Success: the staging bundle is superseded by the curated bundle.
        self.purge_staging(id).await;
        Ok(map)
    }

    async fn reject(
        &self,
        candidate_id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), CurationWorkflowError> {
        let id = candidate_id.trim();
        if id.is_empty() {
            return Err(CurationWorkflowError::ValidationFailed(
                "Candidate ID cannot be empty".to_string(),
            ));
        }

        self.catalog.log_rejection(id, reason, notes).await?;
        self.purge_staging(id).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use crate::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository};
    use crate::domain::{is_perspective_neutral, Candidate, LayerRole, Source};
    use crate::ports::catalog_repository::CatalogRepository;
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

    /// [`CatalogRepository`] double over an in-memory catalog whose `save_curated` always fails,
    /// forcing the curation saga down its rollback path.
    struct FailingSaveCatalog {
        inner: Arc<InMemoryCatalogRepository>,
    }

    #[async_trait]
    impl CatalogRepository for FailingSaveCatalog {
        async fn get_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
            self.inner.get_candidate(id).await
        }

        async fn save_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
            self.inner.save_candidate(candidate).await
        }

        async fn list_candidates(
            &self,
            filter: &crate::ports::catalog_repository::CandidateFilter,
        ) -> Result<Vec<Candidate>, StorageError> {
            self.inner.list_candidates(filter).await
        }

        async fn delete_candidate(&self, id: &str) -> Result<(), StorageError> {
            self.inner.delete_candidate(id).await
        }

        async fn get_curated(&self, id: &str) -> Result<CuratedMap, StorageError> {
            self.inner.get_curated(id).await
        }

        async fn save_curated(&self, _map: &CuratedMap) -> Result<(), StorageError> {
            Err(StorageError::Io(std::io::Error::other(
                "simulated catalog failure",
            )))
        }

        async fn list_curated(
            &self,
            filter: &crate::ports::catalog_repository::CuratedFilter,
        ) -> Result<Vec<CuratedMap>, StorageError> {
            self.inner.list_curated(filter).await
        }

        async fn delete_curated(&self, id: &str) -> Result<(), StorageError> {
            self.inner.delete_curated(id).await
        }

        async fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
            self.inner.is_rejected(id).await
        }

        async fn log_rejection(
            &self,
            id: &str,
            reason: RejectionReason,
            notes: Option<&str>,
        ) -> Result<(), StorageError> {
            self.inner.log_rejection(id, reason, notes).await
        }
    }

    /// [`BlobStorage`] double whose writes fail from the `fail_writes_from`-th write on.
    struct FlakyBlobStorage {
        inner: InMemoryBlobStorage,
        fail_writes_from: usize,
        write_calls: AtomicUsize,
    }

    impl FlakyBlobStorage {
        fn failing_from(fail_writes_from: usize) -> Self {
            Self {
                inner: InMemoryBlobStorage::new(),
                fail_writes_from,
                write_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl BlobStorage for FlakyBlobStorage {
        async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
            self.inner.read_blob(key).await
        }

        async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
            let call = self.write_calls.fetch_add(1, Ordering::SeqCst);
            if call >= self.fail_writes_from {
                return Err(StorageError::Io(std::io::Error::other(
                    "simulated blob write failure",
                )));
            }
            self.inner.write_blob(key, bytes).await
        }

        async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
            self.inner.delete_blob(key).await
        }

        async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
            self.inner.exists_blob(key).await
        }

        async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
            self.inner.list_prefix(prefix).await
        }
    }

    /// Seeds an in-memory catalog and blob store with `candidate` and its staging layer bytes,
    /// addressed exactly as the ports address them (`staging/{id}/{index}.png`).
    async fn staged_ports(
        candidate: Candidate,
        layer_bytes: &[(usize, Vec<u8>)],
    ) -> (Arc<InMemoryCatalogRepository>, Arc<InMemoryBlobStorage>) {
        let catalog = Arc::new(InMemoryCatalogRepository::new());
        let blobs = Arc::new(InMemoryBlobStorage::new());
        catalog.save_candidate(&candidate).await.unwrap();
        for (index, bytes) in layer_bytes {
            blobs
                .write_blob(
                    &BlobKey::StagingLayer {
                        id: candidate.id.clone(),
                        index: *index,
                    },
                    bytes,
                )
                .await
                .unwrap();
        }
        (catalog, blobs)
    }

    fn curated_key(id: &str, index: usize, role: LayerRole, elevation: Option<i8>) -> BlobKey {
        BlobKey::CuratedLayer {
            id: id.to_string(),
            index,
            role,
            elevation,
        }
    }

    /// Canonical paths of every curated blob of `id`, ordered by canonical path.
    async fn curated_paths(blobs: &impl BlobStorage, id: &str) -> Vec<String> {
        blobs
            .list_prefix(&BlobKey::CuratedDir {
                id: Some(id.to_string()),
            })
            .await
            .unwrap()
            .iter()
            .map(|key| key.to_path())
            .collect()
    }

    /// Canonical paths of every staging blob of `id`, ordered by canonical path.
    async fn staging_paths(blobs: &impl BlobStorage, id: &str) -> Vec<String> {
        blobs
            .list_prefix(&BlobKey::StagingDir {
                id: Some(id.to_string()),
            })
            .await
            .unwrap()
            .iter()
            .map(|key| key.to_path())
            .collect()
    }

    fn ground_bytes() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\nfake_ground_bytes".to_vec()
    }

    fn upper_bytes() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\nfake_upper_bytes".to_vec()
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
    async fn test_curate_with_user_description_bypasses_vision_and_persists_every_layer() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        // Unconfigured vision: the user description must bypass it entirely.
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

        let result = service
            .curate(
                "cand_1",
                vec!["dungeon".into(), "crypt".into()],
                None,
                Some("A detailed stone dungeon crypt".into()),
            )
            .await
            .expect("curation should succeed with user description");

        assert_eq!(result.id, "cand_1");
        assert_eq!(result.tags, vec!["dungeon", "crypt"]);
        assert_eq!(result.description.as_deref(), Some("A detailed stone dungeon crypt"));
        assert_eq!(result.layers.len(), 2);
        assert_eq!(result.layers[0].filename, "0_ground.png");
        assert_eq!(
            result.layers[0].caption.as_deref(),
            Some("A detailed stone dungeon crypt")
        );
        assert_eq!(result.layers[1].filename, "1_upper_1.png");

        // The manifest persisted in the catalog is the map returned to the caller.
        assert_eq!(catalog.get_curated("cand_1").await.unwrap(), result);

        // Every included layer was written to its canonical curated blob address with its bytes.
        assert_eq!(
            curated_paths(&blobs, "cand_1").await,
            vec!["curated/cand_1/0_ground.png", "curated/cand_1/1_upper_1.png"]
        );
        assert_eq!(
            blobs
                .read_blob(&curated_key("cand_1", 0, LayerRole::Ground, Some(0)))
                .await
                .unwrap(),
            ground_bytes()
        );
        assert_eq!(
            blobs
                .read_blob(&curated_key("cand_1", 1, LayerRole::UpperFloor, Some(1)))
                .await
                .unwrap(),
            upper_bytes()
        );

        // The candidate left staging: manifest and every staging blob are gone.
        assert!(catalog.get_candidate("cand_1").await.is_err());
        assert!(staging_paths(&blobs, "cand_1").await.is_empty());
    }

    #[tokio::test]
    async fn test_curate_with_vision_description_resolves_and_populates_captions() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        let gen = Arc::new(MockDescriptionGenerator::success(
            "An ancient mossy stone dungeon",
        ));
        let desc_service = Arc::new(DescriptionService::new(Some(gen.clone())));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

        let result = service
            .curate(
                "cand_1",
                vec!["dungeon".into()],
                None,
                None, // Omitted description triggers vision
            )
            .await
            .expect("curation should resolve vision description");

        // One map-level vision call: the undecodable Ground bytes cannot be cropped into sections.
        assert_eq!(gen.calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.description.as_deref(), Some("An ancient mossy stone dungeon"));
        assert_eq!(
            result.layers[0].caption.as_deref(),
            Some("An ancient mossy stone dungeon")
        );
        assert_eq!(catalog.get_curated("cand_1").await.unwrap(), result);
        assert!(catalog.get_candidate("cand_1").await.is_err());
        assert!(staging_paths(&blobs, "cand_1").await.is_empty());
    }

    #[tokio::test]
    async fn test_curate_multi_section_generates_distinct_vision_descriptions() {
        let (cand, ground_png) = two_section_candidate();
        let (catalog, blobs) = staged_ports(cand, &[(0, ground_png)]).await;
        let generator = Arc::new(ContentKeyedGenerator {
            calls: AtomicUsize::new(0),
        });
        let desc_service = Arc::new(DescriptionService::new(Some(generator.clone())));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

        let result = service
            .curate("cand_two", vec!["dungeon".into(), "crypt".into()], None, None)
            .await
            .expect("multi-section curation should succeed");

        // One map-level description plus one per section; the Ground bytes are read once and
        // reused for both crops.
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

        let persisted = catalog.get_curated("cand_two").await.unwrap();
        assert_eq!(persisted.sections, result.sections);
        assert_eq!(persisted.description, result.description);
        assert_eq!(persisted.layers[0].caption, result.description);
    }

    #[tokio::test]
    async fn test_curate_multi_section_failure_reuses_neutral_map_description() {
        let (cand, ground_png) = two_section_candidate();
        let (catalog, blobs) = staged_ports(cand, &[(0, ground_png)]).await;
        let generator = Arc::new(MockDescriptionGenerator::failure(
            DescriptionError::GenerationFailed("OpenRouter 500 error".into()),
        ));
        let desc_service = Arc::new(DescriptionService::new(Some(generator.clone())));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        // A Map Section is a window onto the same encounter space, so a failed crop description
        // reuses the map-level `Physical Description`. Synthesizing a caption here would bake the
        // Trigger Phrase and camera directives into a `Physical Description` (ADR-0002, ADR-0007).
        assert!(
            result
                .sections
                .iter()
                .all(|section| section.description == "Map-level physical description"),
            "Every failed section must reuse the neutral map-level description: {:?}",
            result.sections
        );
        assert!(
            result
                .sections
                .iter()
                .all(|section| is_perspective_neutral(&section.description)),
            "No persisted section description may carry caption or camera directives: {:?}",
            result.sections
        );

        assert_eq!(catalog.get_curated("cand_two").await.unwrap().sections, result.sections);
    }

    #[tokio::test]
    async fn test_curate_vision_failure_propagates_immediately_without_fallback() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        let gen = Arc::new(MockDescriptionGenerator::failure(
            DescriptionError::GenerationFailed("OpenRouter 500 error".into()),
        ));
        let desc_service = Arc::new(DescriptionService::new(Some(gen)));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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

        // Nothing was committed and the staging candidate is untouched.
        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
        assert!(catalog.get_candidate("cand_1").await.is_ok());
        assert_eq!(
            staging_paths(&blobs, "cand_1").await,
            vec!["staging/cand_1/0.png", "staging/cand_1/1.png"]
        );
    }

    #[tokio::test]
    async fn test_curate_unconfigured_vision_fails_when_description_omitted() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None)); // No generator configured
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

        let err = service
            .curate("cand_1", vec!["dungeon".into()], None, None)
            .await
            .unwrap_err();

        match err {
            CurationWorkflowError::DescriptionFailed(DescriptionError::NotConfigured(_)) => {}
            other => panic!("Expected DescriptionFailed(NotConfigured), got {other:?}"),
        }

        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
        assert!(catalog.get_candidate("cand_1").await.is_ok());
    }

    #[tokio::test]
    async fn test_curate_validation_failure_missing_ground_layer() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
        assert!(catalog.get_candidate("cand_1").await.is_ok());
    }

    #[tokio::test]
    async fn test_curate_validation_failure_multiple_ground_layers() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
        assert!(catalog.get_candidate("cand_1").await.is_ok());
    }

    #[tokio::test]
    async fn test_curate_validation_failure_all_excluded() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        let catalog = Arc::new(InMemoryCatalogRepository::new());
        let blobs = Arc::new(InMemoryBlobStorage::new());
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog, blobs, desc_service);

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
    async fn test_curate_custom_layers_curates_only_the_included_subset() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

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
        assert_eq!(
            curated_paths(&blobs, "cand_1").await,
            vec!["curated/cand_1/0_ground.png"]
        );
        // The excluded layer's staging blob is purged with the rest of the staging bundle.
        assert!(staging_paths(&blobs, "cand_1").await.is_empty());
        assert!(catalog.get_candidate("cand_1").await.is_err());
    }

    #[tokio::test]
    async fn test_reject_logs_reason_and_purges_staging() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

        service
            .reject("cand_1", RejectionReason::BlurryLowRes, Some("Too blurry"))
            .await
            .expect("rejection should succeed");

        assert!(catalog.is_rejected("cand_1").await.unwrap());
        assert!(catalog.get_candidate("cand_1").await.is_err());
        assert!(staging_paths(&blobs, "cand_1").await.is_empty());
        // A rejection never produces curated artifacts.
        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
    }

    #[tokio::test]
    async fn test_curate_saga_rolls_back_curated_blobs_when_catalog_commit_fails() {
        let (catalog, blobs) = staged_ports(sample_candidate(), &[(0, ground_bytes()), (1, upper_bytes())]).await;
        let failing: Arc<dyn CatalogRepository + Send + Sync> = Arc::new(FailingSaveCatalog {
            inner: catalog.clone(),
        });
        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(failing, blobs.clone(), desc_service);

        let err = service
            .curate(
                "cand_1",
                vec!["dungeon".into()],
                None,
                Some("A detailed stone dungeon crypt".into()),
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, CurationWorkflowError::Storage(_)),
            "Expected Storage error, got {err:?}"
        );

        // Every curated blob written by the failed call was rolled back …
        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
        // … no manifest was committed …
        assert!(catalog.get_curated("cand_1").await.is_err());
        // … and staging is untouched, so the candidate can be curated again.
        assert!(catalog.get_candidate("cand_1").await.is_ok());
        assert_eq!(
            staging_paths(&blobs, "cand_1").await,
            vec!["staging/cand_1/0.png", "staging/cand_1/1.png"]
        );
    }

    #[tokio::test]
    async fn test_curate_saga_rolls_back_partially_written_blobs_when_a_write_fails() {
        let catalog = Arc::new(InMemoryCatalogRepository::new());
        // Writes 0 and 1 seed staging; write 2 (the Ground curated blob) succeeds and write 3
        // (the UpperFloor curated blob) fails, leaving one curated blob to roll back.
        let blobs = Arc::new(FlakyBlobStorage::failing_from(3));
        catalog.save_candidate(&sample_candidate()).await.unwrap();
        blobs
            .write_blob(&CurationService::staging_key("cand_1", 0), &ground_bytes())
            .await
            .unwrap();
        blobs
            .write_blob(&CurationService::staging_key("cand_1", 1), &upper_bytes())
            .await
            .unwrap();

        let desc_service = Arc::new(DescriptionService::new(None));
        let service = CurationService::new(catalog.clone(), blobs.clone(), desc_service);

        let err = service
            .curate(
                "cand_1",
                vec!["dungeon".into()],
                None,
                Some("A detailed stone dungeon crypt".into()),
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, CurationWorkflowError::Storage(_)),
            "Expected Storage error, got {err:?}"
        );
        assert!(curated_paths(&blobs, "cand_1").await.is_empty());
        assert!(catalog.get_curated("cand_1").await.is_err());
        assert!(catalog.get_candidate("cand_1").await.is_ok());
    }
}
