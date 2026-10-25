use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    Json,
};
use std::sync::atomic::Ordering;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::domain::{
    BatchBenchmarkItem, BenchmarkArchetype, BenchmarkMetadata, BenchmarkModelSpec, Candidate,
    CandidateLayer, normalize_tags, CuratedMap, CuratedMapUpdate, CustomPrompt, DatasetReadiness,
    DatasetSnapshot, LayerRole, LoraAdapter, ModelDescriptor, PurgeStagingResult,
    RedditHarvestRequest, RejectionReason, StorageStats, Theme, TrainingMetricSnapshot, TrainingRun,
};
use crate::services::CreateTrainingRunRequest;
use crate::ports::catalog_repository::{CandidateFilter, CuratedFilter};
use crate::ports::cost_tracker::CostStats;
use crate::services::sections::{describe_map_sections, SectionDescriptionContext};
use crate::services::{
    DatasetExportService, IngestionSummary, LibraryService, RedditIngestionService,
    StorageMaintenanceService,
};
use crate::adapters::clean_tags;
pub use crate::services::description::detect_image_mime;

/// System status response payload.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub staging_count: usize,
    pub curated_count: usize,
    pub uptime_secs: u64,
}

/// GET /api/status - Health check and system metrics.
pub async fn get_status(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    let staging_count = state
        .catalog
        .list_candidates(&CandidateFilter::default())
        .await?
        .len();
    let curated_count = state
        .catalog
        .list_curated(&CuratedFilter::default())
        .await?
        .len();
    let uptime_secs = state.start_time.elapsed().as_secs();

    Ok(Json(StatusResponse {
        status: "ok",
        staging_count,
        curated_count,
        uptime_secs,
    }))
}

/// GET /api/staging - List all pending candidates.
pub async fn get_staging(
    State(state): State<AppState>,
) -> Result<Json<Vec<Candidate>>, ApiError> {
    let candidates = state
        .catalog
        .list_candidates(&CandidateFilter::default())
        .await?;
    Ok(Json(candidates))
}

/// Query parameters for theme-filtered endpoints.
#[derive(Debug, Default, Deserialize)]
pub struct ThemeQuery {
    pub theme: Option<String>,
}

fn parse_optional_theme(theme_str: Option<&str>) -> Result<Option<Theme>, ApiError> {
    match theme_str {
        Some(t) if !t.trim().is_empty() && !t.trim().eq_ignore_ascii_case("all") => {
            let theme = t.trim().parse::<Theme>().map_err(ApiError::bad_request)?;
            Ok(Some(theme))
        }
        _ => Ok(None),
    }
}

/// GET /api/curated - List curated maps, optionally filtered by theme.
pub async fn get_curated(
    State(state): State<AppState>,
    Query(query): Query<ThemeQuery>,
) -> Result<Json<Vec<CuratedMap>>, ApiError> {
    let theme = parse_optional_theme(query.theme.as_deref())?;
    let filter = CuratedFilter {
        theme,
        ..CuratedFilter::default()
    };
    let maps = state.catalog.list_curated(&filter).await?;
    Ok(Json(maps))
}

/// PATCH /api/curated/{id} - Update metadata (themes, tags, description) and/or layer assignments.
pub async fn patch_curated(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut update): Json<CuratedMapUpdate>,
) -> Result<Json<CuratedMap>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Curated map ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());

    if let Some(new_grid) = update.grid {
        let current_map = state.catalog.get_curated(&id).await?;

        let ground_layer = current_map
            .layers
            .iter()
            .find(|l| l.role == LayerRole::Ground)
            .or_else(|| current_map.layers.first())
            .cloned();

        let ground_bytes = library
            .curated_ground_image(&id)
            .await
            .ok()
            .map(|(bytes, _)| bytes);

        let ground_img = ground_bytes.as_ref().and_then(|b| image::load_from_memory(b).ok());
        let (mut ground_w, mut ground_h) = ground_layer
            .as_ref()
            .map(|l| l.dimensions)
            .filter(|&(w, h)| w > 0 && h > 0)
            .unwrap_or(current_map.dimensions);

        if ground_w == 0 || ground_h == 0 {
            if let Some(img) = &ground_img {
                ground_w = img.width();
                ground_h = img.height();
            }
        }

        if !new_grid.is_curation_eligible(ground_w, ground_h) {
            return Err(ApiError::bad_request(
                "New grid configuration yields fewer than 16×16 full Grid Cells",
            ));
        }

        let effective_tags = update.tags.as_ref().unwrap_or(&current_map.tags);
        let effective_desc = update.description.as_deref().or(current_map.description.as_deref());

        // Resolving the map-level Physical Description is strict: it is generated when absent,
        // and single-section maps reuse it verbatim for their sole Map Section.
        let map_description = if let Some(desc) = effective_desc {
            desc.to_string()
        } else {
            let bytes = ground_bytes.as_deref().ok_or_else(|| {
                ApiError::not_found(format!("Ground layer image for '{id}' not found"))
            })?;
            let generated = state
                .description_service
                .describe(
                    bytes,
                    ground_layer.as_ref().map(|l| l.filename.as_str()),
                    effective_tags,
                    None,
                )
                .await?;
            if update.description.is_none() {
                update.description = Some(generated.clone());
            }
            generated
        };

        // Section descriptions share one implementation with curation: vision per crop with a
        // deterministic per-section fallback, reusing the map description for single sections.
        update.sections = Some(
            describe_map_sections(
                &state.description_service,
                SectionDescriptionContext {
                    grid: &new_grid,
                    ground_image: ground_img.as_ref(),
                    ground_dimensions: (ground_w, ground_h),
                    tags: effective_tags,
                    title: None,
                    map_description: &map_description,
                },
            )
            .await,
        );
    }

    let updated = library.update_curated(&id, update).await?;
    tracing::info!(id = %id, "Updated curated map metadata and layer assignments");
    Ok(Json(updated))
}

/// Request payload for deleting a curated map.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeleteCuratedRequest {
    #[serde(default)]
    pub reason: Option<RejectionReason>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// DELETE /api/curated/{id} - Delete an approved curated map bundle and log rejection.
pub async fn delete_curated(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Curated map ID cannot be empty"));
    }

    let req: DeleteCuratedRequest = if body.trim_ascii().is_empty() {
        DeleteCuratedRequest::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_default()
    };

    let reason = req.reason.unwrap_or(RejectionReason::Unspecified);
    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    library
        .delete_curated(&id, reason, req.notes.as_deref())
        .await?;

    tracing::info!(id = %id, reason = ?reason, "Deleted curated map");
    Ok(StatusCode::NO_CONTENT)
}

/// Request payload for bulk deleting curated maps.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BulkDeleteCuratedRequest {
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub reason: Option<RejectionReason>,
}

/// POST /api/curated/bulk-delete - Batch delete curated maps and log rejections.
pub async fn post_curated_bulk_delete(
    State(state): State<AppState>,
    Json(payload): Json<BulkDeleteCuratedRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let reason = payload.reason.unwrap_or(RejectionReason::Unspecified);
    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let mut count = 0;
    for id in &payload.ids {
        if library.delete_curated(id, reason, None).await.is_ok() {
            count += 1;
        }
    }

    tracing::info!(deleted_count = count, total = payload.ids.len(), "Bulk deleted curated maps");
    Ok(Json(json!({ "deleted_count": count })))
}

/// GET /api/dataset/export - Export LoRA dataset packaged as a ZIP archive.
pub async fn get_dataset_export(
    State(state): State<AppState>,
    Query(query): Query<ThemeQuery>,
) -> Result<Response, ApiError> {
    let theme = parse_optional_theme(query.theme.as_deref())?;
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let zip_bytes = export_service.export_lora_dataset(theme.as_ref()).await?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"lora-dataset.zip\""),
        )
        .body(Body::from(zip_bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}

/// GET /api/dataset/readiness - Audit the curated store against the LoRA dataset contract.
pub async fn get_dataset_readiness(
    State(state): State<AppState>,
    Query(query): Query<ThemeQuery>,
) -> Result<Json<DatasetReadiness>, ApiError> {
    let theme = parse_optional_theme(query.theme.as_deref())?;
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let report = export_service.audit_dataset_readiness(theme.as_ref()).await?;

    tracing::info!(
        ready = report.ready,
        maps_ready = report.maps_ready,
        maps_total = report.maps_total,
        sections_ready = report.sections_ready,
        sections_total = report.sections_total,
        "Audited LoRA dataset readiness"
    );

    Ok(Json(report))
}

/// Request payload for exporting selected curated maps to a LoRA dataset ZIP archive.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExportSelectedRequest {
    #[serde(default)]
    pub ids: Vec<String>,
}

/// POST /api/dataset/export-selected - Export selected curated maps packaged as a ZIP archive.
pub async fn post_dataset_export_selected(
    State(state): State<AppState>,
    Json(payload): Json<ExportSelectedRequest>,
) -> Result<Response, ApiError> {
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let zip_bytes = export_service
        .export_selected_lora_dataset(&payload.ids)
        .await?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"lora-dataset.zip\""),
        )
        .body(Body::from(zip_bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}

/// Request payload for freezing an immutable dataset snapshot.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FreezeDatasetRequest {
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
}

/// POST /api/training/datasets - Freeze a scope into an immutable DatasetSnapshot.
pub async fn post_training_dataset(
    State(state): State<AppState>,
    Query(query): Query<ThemeQuery>,
    body: Option<Json<FreezeDatasetRequest>>,
) -> Result<(StatusCode, Json<DatasetSnapshot>), ApiError> {
    let req = body.map(|b| b.0).unwrap_or_default();
    let theme_str = req.theme.as_deref().or(query.theme.as_deref());
    let theme = parse_optional_theme(theme_str)?;
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let snapshot = export_service
        .freeze_dataset_snapshot(theme.as_ref(), req.id)
        .await?;
    tracing::info!(
        id = %snapshot.id,
        sha256 = %snapshot.sha256,
        samples = snapshot.samples_count,
        maps = snapshot.maps_count,
        "Frozen dataset snapshot created"
    );
    Ok((StatusCode::CREATED, Json(snapshot)))
}

/// GET /api/training/datasets - List frozen dataset snapshots, optionally filtered by theme.
pub async fn get_training_datasets(
    State(state): State<AppState>,
    Query(query): Query<ThemeQuery>,
) -> Result<Json<Vec<DatasetSnapshot>>, ApiError> {
    let theme = parse_optional_theme(query.theme.as_deref())?;
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let snapshots = export_service.list_dataset_snapshots(theme.as_ref()).await?;
    Ok(Json(snapshots))
}

/// GET /api/training/datasets/{id} - Get frozen dataset snapshot metadata by ID.
pub async fn get_training_dataset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DatasetSnapshot>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Snapshot ID cannot be empty"));
    }
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let snapshot = export_service.get_dataset_snapshot(&id).await?;
    Ok(Json(snapshot))
}

/// GET /api/training/datasets/{id}/archive - Download the frozen dataset snapshot ZIP archive.
pub async fn get_training_dataset_archive(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Snapshot ID cannot be empty"));
    }
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    let bytes = export_service.read_dataset_snapshot_archive(&id).await?;
    let filename = format!("{id}.zip");
    let disposition = format!("attachment; filename=\"{filename}\"");

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::try_from(disposition).map_err(|e| ApiError::internal(e.to_string()))?,
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}

/// DELETE /api/training/datasets/{id} - Delete an unreferenced dataset snapshot and its blob.
pub async fn delete_training_dataset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Snapshot ID cannot be empty"));
    }
    let export_service = DatasetExportService::new(state.catalog.clone(), state.blobs.clone());
    export_service.delete_dataset_snapshot(&id).await?;
    tracing::info!(id = %id, "Deleted dataset snapshot");
    Ok(Json(json!({ "deleted": true, "id": id })))
}

/// Request payload for publishing an adapter from a training run checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishAdapterRequest {
    pub step: usize,
    #[serde(default)]
    pub scale: Option<f64>,
}

/// Query parameters for listing LoRA adapters.
#[derive(Debug, Default, Deserialize)]
pub struct ListAdaptersQuery {
    #[serde(default)]
    pub selectable: Option<bool>,
    #[serde(default)]
    pub usable: Option<bool>,
}

fn default_true() -> bool {
    true
}

/// Request payload for soft-retiring or restoring a LoRA adapter.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetireAdapterRequest {
    #[serde(default = "default_true")]
    pub retired: bool,
}

/// POST /api/training/runs - Create a training run in queued state.
pub async fn post_training_run(
    State(state): State<AppState>,
    Json(payload): Json<CreateTrainingRunRequest>,
) -> Result<(StatusCode, Json<TrainingRun>), ApiError> {
    let run = state.training_service.create_training_run(payload).await?;
    tracing::info!(id = %run.id, "Created training run in queued state");
    Ok((StatusCode::CREATED, Json(run)))
}

/// GET /api/training/runs - List all training runs.
pub async fn get_training_runs(
    State(state): State<AppState>,
) -> Result<Json<Vec<TrainingRun>>, ApiError> {
    let runs = state.training_service.list_runs().await?;
    Ok(Json(runs))
}

/// GET /api/training/runs/{id} - Get training run detail and status.
pub async fn get_training_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrainingRun>, ApiError> {
    let run = state.training_service.get_run(&id).await?;
    Ok(Json(run))
}

/// POST /api/training/runs/{id}/dispatch - Start a queued training run on Modal.
pub async fn post_training_run_dispatch(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrainingRun>, ApiError> {
    let run = state.training_service.dispatch_run(&id).await?;
    tracing::info!(id = %run.id, "Dispatched training run to runner");
    Ok(Json(run))
}

/// POST /api/training/runs/{id}/cancel - Cancel an active training run.
pub async fn post_training_run_cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrainingRun>, ApiError> {
    let run = state.training_service.cancel_run(&id).await?;
    tracing::info!(id = %run.id, "Cancelled training run");
    Ok(Json(run))
}

/// POST /api/training/runs/{id}/publish - Publish an adapter descriptor from a checkpoint.
pub async fn post_training_run_publish(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<PublishAdapterRequest>,
) -> Result<Json<LoraAdapter>, ApiError> {
    let adapter = state
        .training_service
        .publish_adapter(&id, payload.step, payload.scale)
        .await?;
    tracing::info!(adapter_id = %adapter.id, run_id = %id, step = payload.step, "Published LoRA adapter");
    Ok(Json(adapter))
}

/// POST /api/training/runs/{id}/verify - Verify checkpoint artifacts in storage.
pub async fn post_training_run_verify(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrainingRun>, ApiError> {
    let run = state.training_service.verify_run_artifacts(&id).await?;
    Ok(Json(run))
}

/// GET /api/training/runs/{id}/metrics - Get chronological metric snapshots for a training run.
pub async fn get_training_run_metrics(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TrainingMetricSnapshot>>, ApiError> {
    let metrics = state.training_service.get_run_metrics(&id).await?;
    Ok(Json(metrics))
}

/// GET /api/training/adapters - List published LoRA adapter descriptors.
pub async fn get_training_adapters(
    State(state): State<AppState>,
    Query(query): Query<ListAdaptersQuery>,
) -> Result<Json<Vec<LoraAdapter>>, ApiError> {
    let selectable_only = query.selectable.or(query.usable).unwrap_or(false);
    let adapters = state.training_service.list_adapters(selectable_only).await?;
    Ok(Json(adapters))
}

/// GET /api/training/adapters/{id} - Get one LoRA adapter descriptor.
pub async fn get_training_adapter(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<LoraAdapter>, ApiError> {
    let adapter = state.training_service.get_adapter(&id).await?;
    Ok(Json(adapter))
}

/// POST /api/training/adapters/{id}/retire - Soft-retire or restore a LoRA adapter.
pub async fn post_retire_training_adapter(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<RetireAdapterRequest>>,
) -> Result<Json<LoraAdapter>, ApiError> {
    let retired = body.map(|b| b.retired).unwrap_or(true);
    let adapter = state.training_service.set_adapter_retired(&id, retired).await?;
    tracing::info!(id = %id, retired = %retired, "Updated LoRA adapter retired status");
    Ok(Json(adapter))
}

/// GET /api/image/staging/{id} - Stream binary image bytes with MIME type and Cache-Control.
pub async fn get_staging_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = library.staging_image(&id).await?;
    let mime = detect_image_mime(&bytes, Some(&filename));

    Ok(image_response(bytes, mime))
}

/// GET /api/image/staging/{id}/{layer_index} - Stream binary image bytes for a specific layer of a staged candidate.
pub async fn get_staging_layer_image(
    State(state): State<AppState>,
    Path((id, layer_index)): Path<(String, usize)>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = library.staging_layer_image(&id, layer_index).await?;
    let mime = detect_image_mime(&bytes, filename.as_deref());

    Ok(image_response(bytes, mime))
}

/// Builds an image response with the same MIME type and cache headers every image route emits.
fn image_response(bytes: Vec<u8>, mime: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// GET /api/image/curated/{id} - Stream the Ground layer image of a curated map by ID.
pub async fn get_curated_image_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Map ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = library.curated_ground_image(&id).await?;
    let mime = detect_image_mime(&bytes, filename.as_deref());

    Ok(image_response(bytes, mime))
}

/// GET /api/image/curated/{id}/{layer_index} - Stream binary image bytes for a specific layer of a curated map.
pub async fn get_curated_layer_image_by_id(
    State(state): State<AppState>,
    Path((id, layer_index)): Path<(String, usize)>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Map ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = library.curated_layer_image(&id, layer_index).await?;
    let mime = detect_image_mime(&bytes, filename.as_deref());

    Ok(image_response(bytes, mime))
}

/// Dispatcher for GET /api/image/curated/{id}/{layer_index}: the second segment is always a
/// layer index now that themes are a filter concept rather than a storage path.
pub async fn get_curated_image_or_layer(
    state: State<AppState>,
    Path((id, selector)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let layer_index = selector.parse::<usize>().map_err(|_| {
        ApiError::not_found(format!(
            "Curated image layer '{selector}' for '{id}' not found"
        ))
    })?;
    get_curated_layer_image_by_id(state, Path((id, layer_index))).await
}


/// Request payload for curating a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateRequest {
    pub id: String,
    #[serde(default)]
    pub themes: Option<Vec<String>>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub rating: Option<u8>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub layers: Option<Vec<CandidateLayer>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl CurateRequest {
    pub fn resolved_tags(&self) -> Vec<String> {
        let mut raw: Vec<String> = self.themes.clone().unwrap_or_default();

        if let Some(theme) = &self.theme {
            let trimmed = theme.trim();
            if !trimmed.is_empty() {
                raw.push(
                    trimmed
                        .parse::<Theme>()
                        .map(|th| th.as_str().to_string())
                        .unwrap_or_else(|_| trimmed.to_string()),
                );
            }
        }

        raw.extend(self.tags.clone().unwrap_or_default());
        normalize_tags(raw)
    }
}

/// Request payload for updating candidate layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLayersRequest {
    pub layers: Vec<CandidateLayer>,
}

/// Response payload for curate endpoint.
#[derive(Debug, Serialize)]
pub struct CurateResponse {
    pub status: &'static str,
    pub curated: CuratedMap,
}

/// POST /api/curate - Move candidate to curated directory with dynamic themes and tags.
pub async fn post_curate(
    State(state): State<AppState>,
    Json(payload): Json<CurateRequest>,
) -> Result<Json<CurateResponse>, ApiError> {
    if payload.id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let resolved_tags = payload.resolved_tags();
    if resolved_tags.is_empty() {
        return Err(ApiError::bad_request("At least 1 tag is required"));
    }
    let curated = state
        .curation_service
        .curate(
            &payload.id,
            resolved_tags,
            payload.layers,
            payload.description,
        )
        .await?;

    tracing::info!(
        id = %curated.id,
        tags = ?curated.tags,
        "Curated candidate image"
    );
    Ok(Json(CurateResponse {
        status: "ok",
        curated,
    }))
}

/// PATCH /api/staging/{id}/layers - Update candidate layer metadata in staging manifest.
pub async fn patch_staging_layers(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateLayersRequest>,
) -> Result<Json<Candidate>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let updated_candidate = library.update_staging_layers(&id, payload.layers).await?;

    Ok(Json(updated_candidate))
}
/// PATCH /api/staging/{id}/grid - Update candidate grid configuration in staging manifest.
pub async fn patch_staging_grid(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<crate::domain::GridConfig>,
) -> Result<Json<Candidate>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let updated_candidate = library.update_staging_grid(&id, payload).await?;

    Ok(Json(updated_candidate))
}

/// POST /api/staging/{id}/split - Decompose a multi-layer bundle into standalone candidates.
pub async fn post_split_staging(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Candidate>>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let split_candidates = library.split_staging_bundle(&id).await?;

    Ok(Json(split_candidates))
}

/// POST /api/staging/{parent_id}/regroup - Recombine split candidates back into a single bundle.
pub async fn post_regroup_staging(
    State(state): State<AppState>,
    Path(parent_id): Path<String>,
) -> Result<Json<Candidate>, ApiError> {
    if parent_id.trim().is_empty() {
        return Err(ApiError::bad_request("Parent ID cannot be empty"));
    }

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let regrouped_candidate = library.regroup_staging_bundle(&parent_id).await?;

    Ok(Json(regrouped_candidate))
}

/// Response payload for theme suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuggestThemesResponse {
    pub id: String,
    pub suggestions: Vec<String>,
}

/// POST /api/staging/{id}/suggest-themes - Vision-assisted theme tag suggestions with caching.
pub async fn post_suggest_themes(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuggestThemesResponse>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    // 1. Fast in-memory cache check
    {
        let cache = state.theme_suggestions_cache.read().await;
        if let Some(suggestions) = cache.get(&id) {
            return Ok(Json(SuggestThemesResponse {
                id,
                suggestions: suggestions.clone(),
            }));
        }
    }

    // 2. Load candidate image bytes from staging storage
    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = match library.staging_image(&id).await {
        Ok(loaded) => loaded,
        Err(err) => {
            tracing::warn!(candidate_id = %id, error = %err, "Candidate image not found in staging; returning empty suggestions");
            return Ok(Json(SuggestThemesResponse {
                id,
                suggestions: Vec::new(),
            }));
        }
    };
    let mime = detect_image_mime(&bytes, Some(&filename));

    // 3. Invoke vision model if OpenRouter adapter is available
    let raw_suggestions = match state.openrouter_client() {
        Some(adapter) => match adapter.suggest_themes(&bytes, mime).await {
            Ok(tags) => tags,
            Err(err) => {
                tracing::warn!(candidate_id = %id, error = %err, "OpenRouter vision model call failed; falling back to empty suggestions");
                Vec::new()
            }
        },
        None => {
            tracing::warn!(candidate_id = %id, "OpenRouter vision model not configured or missing API key; returning empty suggestions");
            Vec::new()
        }
    };

    // 4. Clean up tags (trim, deduplicate, capitalize, limit to 5)
    let suggestions = clean_tags(raw_suggestions);

    // 5. Store non-empty results in cache
    if !suggestions.is_empty() {
        state
            .theme_suggestions_cache
            .write()
            .await
            .insert(id.clone(), suggestions.clone());
    }

    Ok(Json(SuggestThemesResponse { id, suggestions }))
}

/// POST /api/curated/{id}/suggest-themes - Vision-assisted theme tag suggestions for curated map with caching.
pub async fn post_curated_suggest_themes(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuggestThemesResponse>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Curated map ID cannot be empty"));
    }

    // 1. Fast in-memory cache check
    {
        let cache = state.theme_suggestions_cache.read().await;
        if let Some(suggestions) = cache.get(&id) {
            return Ok(Json(SuggestThemesResponse {
                id,
                suggestions: suggestions.clone(),
            }));
        }
    }

    // 2. Load primary ground image bytes
    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = library
        .curated_ground_image(&id)
        .await
        .map_err(|_| ApiError::not_found(format!("Curated map '{id}' not found")))?;
    let mime = detect_image_mime(&bytes, filename.as_deref());

    // 3. Invoke vision model if OpenRouter adapter is available
    let raw_suggestions = match state.openrouter_client() {
        Some(adapter) => match adapter.suggest_themes(&bytes, mime).await {
            Ok(tags) => tags,
            Err(err) => {
                tracing::warn!(curated_id = %id, error = %err, "OpenRouter vision model call failed; falling back to empty suggestions");
                Vec::new()
            }
        },
        None => {
            tracing::warn!(curated_id = %id, "OpenRouter vision model not configured or missing API key; returning empty suggestions");
            Vec::new()
        }
    };

    // 4. Clean up tags (trim, deduplicate, capitalize, limit to 5)
    let suggestions = clean_tags(raw_suggestions);

    // 5. Store non-empty results in cache
    if !suggestions.is_empty() {
        state
            .theme_suggestions_cache
            .write()
            .await
            .insert(id.clone(), suggestions.clone());
    }

    Ok(Json(SuggestThemesResponse { id, suggestions }))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DescribeRequest {
    #[serde(default)]
    pub themes: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DescribeResponse {
    pub id: String,
    pub description: String,
}

/// POST /api/staging/{id}/describe or GET /api/staging/{id}/describe
/// Generate a rich, physical battlemap description from candidate image and optional themes/tags.
pub async fn post_describe_candidate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<DescribeResponse>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let req: DescribeRequest = if body.trim_ascii().is_empty() {
        DescribeRequest::default()
    } else {
        serde_json::from_slice::<Option<DescribeRequest>>(&body)
            .map_err(|e| ApiError::bad_request(format!("Invalid JSON payload: {e}")))?
            .unwrap_or_default()
    };

    // 1. Locate staging candidate
    let candidate = state
        .catalog
        .get_candidate(&id)
        .await
        .map_err(|_| ApiError::not_found(format!("Candidate '{id}' not found in staging")))?;

    // 2. Read primary image bytes (from candidate layer 0 or candidate primary image)
    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, _) = library.staging_image(&id).await?;
    let filename = candidate.resolved_filename();

    // 3. Resolve themes and tags (prefer payload themes/tags, fallback to cached suggestions).
    // Themes and tags share one normalized tag vocabulary in the vision prompt.
    let mut themes = req.themes.unwrap_or_default();
    if themes.is_empty() {
        let cache = state.theme_suggestions_cache.read().await;
        if let Some(cached) = cache.get(&id) {
            themes = cached.clone();
        }
    }
    let tags = normalize_tags(themes.into_iter().chain(req.tags.unwrap_or_default()));

    let description = state
        .description_service
        .describe(
            &bytes,
            Some(&filename),
            &tags,
            candidate.title.as_deref(),
        )
        .await?;

    Ok(Json(DescribeResponse { id, description }))
}

/// POST /api/curated/{id}/describe or GET /api/curated/{id}/describe
/// Generate physical description from curated map image and optional themes/tags.
pub async fn post_curated_describe(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<DescribeResponse>, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Curated map ID cannot be empty"));
    }

    let req: DescribeRequest = if body.trim_ascii().is_empty() {
        DescribeRequest::default()
    } else {
        serde_json::from_slice::<Option<DescribeRequest>>(&body)
            .map_err(|e| ApiError::bad_request(format!("Invalid JSON payload: {e}")))?
            .unwrap_or_default()
    };

    let curated_map = state.catalog.get_curated(&id).await.ok();

    let library = LibraryService::new(state.catalog.clone(), state.blobs.clone());
    let (bytes, filename) = library
        .curated_ground_image(&id)
        .await
        .map_err(|_| ApiError::not_found(format!("Curated map '{id}' not found")))?;
    let filename = filename.as_deref();

    let mut themes = req.themes.unwrap_or_default();
    if themes.is_empty() {
        if let Some(m) = &curated_map {
            themes = m.tags.clone();
        }
    }
    if themes.is_empty() {
        let cache = state.theme_suggestions_cache.read().await;
        if let Some(cached) = cache.get(&id) {
            themes = cached.clone();
        }
    }
    let mut requested_tags = req.tags.unwrap_or_default();
    if requested_tags.is_empty() {
        if let Some(m) = &curated_map {
            requested_tags = m.tags.clone();
        }
    }
    let tags = normalize_tags(themes.into_iter().chain(requested_tags));

    // `CuratedMap` has no standalone title field, so no title is forwarded. The stored
    // description is generation output and is never an input to its own regeneration.
    let description = state
        .description_service
        .describe(&bytes, filename, &tags, None)
        .await?;

    Ok(Json(DescribeResponse { id, description }))
}

/// Request payload for rejecting a candidate.
#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    pub id: String,
    #[serde(default)]
    pub reason: Option<RejectionReason>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Response payload for reject endpoint.
#[derive(Debug, Serialize)]
pub struct RejectResponse {
    pub status: &'static str,
    pub id: String,
}

/// POST /api/reject - Remove candidate from staging and log rejection.
pub async fn post_reject(
    State(state): State<AppState>,
    Json(payload): Json<RejectRequest>,
) -> Result<Json<RejectResponse>, ApiError> {
    if payload.id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let reason = payload.reason.unwrap_or_default();

    state
        .curation_service
        .reject(&payload.id, reason, payload.notes.as_deref())
        .await?;

    tracing::info!(
        id = %payload.id,
        reason = ?reason,
        "Rejected candidate image"
    );
    Ok(Json(RejectResponse {
        status: "ok",
        id: payload.id,
    }))
}

/// Request payload for undoing a candidate rejection.
#[derive(Debug, Deserialize, Default)]
pub struct UndoRejectRequest {
    #[serde(default)]
    pub id: Option<String>,
}

/// Response payload for undo reject endpoint.
#[derive(Debug, Serialize)]
pub struct UndoRejectResponse {
    pub status: &'static str,
    pub candidate: Candidate,
}

/// POST /api/reject/undo - Restore a rejected candidate to the staging queue.
pub async fn post_undo_reject(
    State(state): State<AppState>,
    Json(payload): Json<UndoRejectRequest>,
) -> Result<Json<UndoRejectResponse>, ApiError> {
    let candidate = state
        .curation_service
        .undo_reject(payload.id.as_deref())
        .await?;

    tracing::info!(
        id = %candidate.id,
        "Candidate rejection undone, restored to staging"
    );

    Ok(Json(UndoRejectResponse {
        status: "restored",
        candidate,
    }))
}

/// Response payload for harvest endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedditHarvestResponse {
    pub processed: usize,
    pub downloaded: usize,
    pub skipped: usize,
    pub summary: IngestionSummary,
}

impl From<IngestionSummary> for RedditHarvestResponse {
    fn from(s: IngestionSummary) -> Self {
        Self {
            processed: s.processed,
            downloaded: s.downloaded,
            skipped: s.skipped,
            summary: s,
        }
    }
}

/// POST /api/harvest/reddit - Harvest battlemaps from Reddit with live SSE streaming or JSON summary.
pub async fn post_harvest_reddit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RedditHarvestRequest>,
) -> Result<Response, ApiError> {
    let is_sse = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let query = payload.to_query();
    let gridless_only = payload.gridless_only;
    let service = RedditIngestionService::new(
        state.harvester.clone(),
        state.catalog.clone(),
        state.blobs.clone(),
    );

    tracing::info!(
        subreddit = %query.subreddit,
        sort = %query.sort,
        limit = query.limit,
        streaming = if is_sse { "SSE" } else { "JSON" },
        "Handling Reddit harvest request"
    );

    if is_sse {
        let (tx, rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<Event, std::convert::Infallible>>();
        tokio::spawn(async move {
            let res = service
                .ingest_query_progress(&query, gridless_only, |progress| {
                    let json_data = serde_json::to_string(&progress).unwrap_or_default();
                    let event_name = if progress.phase == "complete" {
                        "complete"
                    } else {
                        "progress"
                    };
                    let event = Event::default().event(event_name).data(json_data);
                    let _ = tx.send(Ok(event));
                })
                .await;

            if let Err(err) = res {
                let err_event = Event::default().event("error").data(
                    serde_json::to_string(&serde_json::json!({ "error": err.to_string() }))
                        .unwrap_or_default(),
                );
                let _ = tx.send(Ok(err_event));
            }
        });

        let stream = UnboundedReceiverStream::new(rx);
        Ok(Sse::new(stream).into_response())
    } else {
        let summary = service.ingest_query_filtered(&query, gridless_only).await?;
        let response = RedditHarvestResponse::from(summary);
        Ok(Json(response).into_response())
    }
}

/// Response payload for resetting the session spend counter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetSessionResponse {
    pub status: &'static str,
    pub session_spend_usd: f64,
}

/// GET /api/cost/stats - Returns telemetry, session spend, cumulative spend, and per-model breakdown.
pub async fn get_cost_stats(
    State(state): State<AppState>,
) -> Result<Json<CostStats>, ApiError> {
    let stats = state
        .cost_tracker
        .get_stats()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(stats))
}

/// POST /api/cost/reset-session - Resets the in-memory session spend counters.
pub async fn post_reset_cost_session(
    State(state): State<AppState>,
) -> Result<Json<ResetSessionResponse>, ApiError> {
    state
        .cost_tracker
        .reset_session()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(ResetSessionResponse {
        status: "ok",
        session_spend_usd: 0.0,
    }))
}

/// GET /api/models - Returns the list of selectable model descriptors (ADR-0010).
pub async fn get_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<ModelDescriptor>>, ApiError> {
    let models = state.model_registry.list_selectable().await?;
    Ok(Json(models))
}

/// Request payload for initiating a batch AI benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunRequest {
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub archetypes: Vec<String>,
    #[serde(default)]
    pub custom_prompts: Vec<CustomPrompt>,
    #[serde(default)]
    pub use_reference_conditioning: Option<bool>,
    #[serde(default)]
    pub use_reference_image: Option<bool>,
    #[serde(default)]
    pub reference_conditioning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lora_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lora_scale: Option<f64>,
}

impl BenchmarkRunRequest {
    pub fn explicit_reference_conditioning(&self) -> Option<bool> {
        self.use_reference_conditioning
            .or(self.use_reference_image)
            .or(self.reference_conditioning)
    }

    pub fn is_reference_conditioning_requested(&self) -> bool {
        self.explicit_reference_conditioning() == Some(true)
    }

    pub fn reference_conditioning_enabled(&self) -> bool {
        self.explicit_reference_conditioning().unwrap_or(true)
    }
}

/// Response payload for completed non-streaming benchmark runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunResponse {
    pub status: &'static str,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<BenchmarkMetadata>,
    pub total_cost_usd: f64,
}

/// Response payload for benchmark cancellation requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCancelResponse {
    pub status: &'static str,
    pub message: &'static str,
}

/// POST /api/benchmark/run - Execute multi-model, multi-prompt benchmarks with SSE or JSON output.
pub async fn post_benchmark_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<BenchmarkRunRequest>,
) -> Result<Response, ApiError> {
    if payload.models.is_empty() {
        return Err(ApiError::bad_request("At least one model must be selected"));
    }

    if payload.archetypes.is_empty() && payload.custom_prompts.is_empty() {
        return Err(ApiError::bad_request(
            "At least one benchmark archetype or custom prompt must be selected",
        ));
    }

    // Resolve items from archetypes and custom prompts
    let mut items = Vec::new();
    for arch_str in &payload.archetypes {
        let trimmed = arch_str.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(arch) = trimmed.parse::<BenchmarkArchetype>() {
            items.push(BatchBenchmarkItem::from_archetype(&arch));
        } else {
            items.push(BatchBenchmarkItem::new(trimmed, trimmed));
        }
    }
    for custom in &payload.custom_prompts {
        if !custom.prompt.trim().is_empty() {
            items.push(BatchBenchmarkItem::from_custom(custom));
        }
    }

    if items.is_empty() {
        return Err(ApiError::bad_request(
            "No valid archetypes or custom prompts provided",
        ));
    }

    // Model validation and reference conditioning capability gate (ADR-0010)
    for model_id in &payload.models {
        let selectable_desc = state.model_registry.find(model_id).await?;
        let desc = match selectable_desc {
            Some(d) => d,
            None => {
                if let Ok(Some(_)) = state.model_registry.find_including_retired(model_id).await {
                    return Err(ApiError::bad_request(format!(
                        "Model '{model_id}' is retired or unselectable for new benchmark runs"
                    )));
                } else if let Some(spec) = BenchmarkModelSpec::find(model_id) {
                    spec.into()
                } else {
                    return Err(ApiError::bad_request(format!(
                        "Unknown or unselectable benchmark model: '{model_id}'"
                    )));
                }
            }
        };

        if payload.reference_conditioning_enabled() && !desc.supports_reference_image {
            return Err(ApiError::model_not_reference_capable(model_id));
        }
    }

    let mut service = state
        .get_or_create_benchmark_service()?
        .as_ref()
        .clone()
        .with_reference_conditioning(payload.reference_conditioning_enabled());

    if let Some(path) = payload.lora_path.filter(|p| !p.trim().is_empty()) {
        let scale = payload.lora_scale.unwrap_or(0.75);
        service = service.with_lora(path, scale);
    }

    let is_sse = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let models = payload.models;
    let cancel_flag = state.benchmark_cancelled.clone();
    cancel_flag.store(false, Ordering::Relaxed);

    if is_sse {
        let (tx, rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<Event, std::convert::Infallible>>();
        let flag_clone = cancel_flag.clone();

        tokio::spawn(async move {
            let res = service
                .run_batch_with_progress(&models, &items, Some(flag_clone), |progress| {
                    if tx.is_closed() {
                        return false;
                    }
                    let json_data = serde_json::to_string(&progress).unwrap_or_default();
                    let event_name = match progress.phase.as_str() {
                        "complete" => "complete",
                        "cancelled" => "cancelled",
                        "error" => "error",
                        _ => "progress",
                    };
                    let event = Event::default().event(event_name).data(json_data);
                    tx.send(Ok(event)).is_ok()
                })
                .await;

            if let Err(err) = res {
                if !tx.is_closed() {
                    let err_event = Event::default().event("error").data(
                        serde_json::to_string(&serde_json::json!({ "error": err.to_string() }))
                            .unwrap_or_default(),
                    );
                    let _ = tx.send(Ok(err_event));
                }
            }
        });

        let stream = UnboundedReceiverStream::new(rx);
        Ok(Sse::new(stream).into_response())
    } else {
        let results = service
            .run_batch_with_progress(&models, &items, Some(cancel_flag), |_| true)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let total = models.len() * items.len();
        let completed = results.len();
        let total_cost_usd = results.iter().map(|r| r.metadata.cost_usd).sum();
        let metadatas = results.into_iter().map(|r| r.metadata).collect();

        let response = BenchmarkRunResponse {
            status: "ok",
            total,
            completed,
            results: metadatas,
            total_cost_usd,
        };

        Ok(Json(response).into_response())
    }
}

/// POST /api/benchmark/cancel - Request mid-flight abort of active benchmark execution.
pub async fn post_benchmark_cancel(
    State(state): State<AppState>,
) -> Result<Json<BenchmarkCancelResponse>, ApiError> {
    state.benchmark_cancelled.store(true, Ordering::Relaxed);
    Ok(Json(BenchmarkCancelResponse {
        status: "ok",
        message: "Benchmark batch cancellation requested",
    }))
}

/// GET /api/storage/stats - Returns comprehensive storage quota and usage statistics.
pub async fn get_storage_stats(
    State(state): State<AppState>,
) -> Result<Json<StorageStats>, ApiError> {
    let maintenance = StorageMaintenanceService::new(state.catalog.clone(), state.blobs.clone());
    let stats = maintenance.storage_stats().await?;
    Ok(Json(stats))
}

/// POST /api/storage/purge-staging - Purges all unreviewed candidates and files from the staging area.
pub async fn post_purge_staging(
    State(state): State<AppState>,
) -> Result<Json<PurgeStagingResult>, ApiError> {
    let maintenance = StorageMaintenanceService::new(state.catalog.clone(), state.blobs.clone());
    let result = maintenance.purge_staging().await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_image_mime() {
        assert_eq!(
            detect_image_mime(b"\x89PNG\r\n\x1a\nremaining_data", None),
            "image/png"
        );
        assert_eq!(
            detect_image_mime(b"\xFF\xD8\xFFremaining_data", None),
            "image/jpeg"
        );
        assert_eq!(
            detect_image_mime(b"RIFF\x00\x00\x00\x00WEBPVP8 ", None),
            "image/webp"
        );
        assert_eq!(
            detect_image_mime(b"GIF89a\x00\x00", None),
            "image/gif"
        );
        assert_eq!(
            detect_image_mime(b"unknown_bytes", Some("map.jpg")),
            "image/jpeg"
        );
        assert_eq!(
            detect_image_mime(b"unknown_bytes", Some("map.webp")),
            "image/webp"
        );
        assert_eq!(
            detect_image_mime(b"unknown_bytes", None),
            "image/png"
        );
    }
}
