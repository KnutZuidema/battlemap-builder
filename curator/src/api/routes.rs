use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    Json,
};
use std::sync::atomic::Ordering;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::domain::{
    BatchBenchmarkItem, BenchmarkArchetype, BenchmarkMetadata, Candidate, CandidateLayer,
    CuratedMap, CustomPrompt, RedditHarvestRequest, RejectionReason, Theme,
};
use crate::ports::cost_tracker::CostStats;
use crate::services::{IngestionSummary, RedditIngestionService};
/// Inspects bytes and optional filename to determine image MIME type.
pub fn detect_image_mime(bytes: &[u8], filename: Option<&str>) -> &'static str {
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        return "image/png";
    }
    if bytes.len() >= 3 && &bytes[0..3] == b"\xFF\xD8\xFF" {
        return "image/jpeg";
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    if bytes.len() >= 6 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        return "image/gif";
    }

    if let Some(name) = filename {
        if let Some(ext) = std::path::Path::new(name).extension().and_then(|e| e.to_str()) {
            match ext.to_lowercase().as_str() {
                "jpg" | "jpeg" => return "image/jpeg",
                "webp" => return "image/webp",
                "gif" => return "image/gif",
                "png" => return "image/png",
                _ => {}
            }
        }
    }

    "image/png"
}

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
    let staging_count = state.storage.list_staging()?.len();
    let curated_count = state.storage.list_curated(None)?.len();
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
    let candidates = state.storage.list_staging()?;
    Ok(Json(candidates))
}

/// GET /api/image/staging/{id} - Stream binary image bytes with MIME type and Cache-Control.
pub async fn get_staging_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let bytes = state.storage.get_staging_image(&id)?;

    // Try finding candidate in staging to inspect filename for MIME
    let filename = state
        .storage
        .list_staging()
        .ok()
        .and_then(|list| list.into_iter().find(|c| c.id == id).map(|c| c.filename));

    let mime = detect_image_mime(&bytes, filename.as_deref());

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}
/// GET /api/image/staging/{id}/{layer_index} - Stream binary image bytes for a specific layer of a staged candidate.
pub async fn get_staging_layer_image(
    State(state): State<AppState>,
    Path((id, layer_index)): Path<(String, usize)>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    let bytes = state.storage.get_staging_layer_image(&id, layer_index)?;

    // Inspect candidate layers to detect MIME type from layer filename
    let filename = state
        .storage
        .list_staging()
        .ok()
        .and_then(|list| {
            list.into_iter().find(|c| c.id == id).and_then(|c| {
                let layers = c.resolved_layers();
                layers
                    .into_iter()
                    .find(|l| l.index == layer_index)
                    .map(|l| l.filename)
                    .or(if layer_index == 0 {
                        Some(c.filename)
                    } else {
                        None
                    })
            })
        });

    let mime = detect_image_mime(&bytes, filename.as_deref());

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}


/// GET /api/image/curated/{theme}/{id} - Stream binary image for a curated map.
pub async fn get_curated_image(
    State(state): State<AppState>,
    Path((theme_str, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Map ID cannot be empty"));
    }

    let theme: Theme = theme_str
        .parse::<Theme>()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let bytes = state.storage.get_curated_image(&theme, &id)?;
    let mime = detect_image_mime(&bytes, None);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}
/// GET /api/image/curated/{theme}/{id}/{layer_index} - Stream binary image bytes for a specific layer of a curated map.
pub async fn get_curated_layer_image(
    State(state): State<AppState>,
    Path((theme_str, id, layer_index)): Path<(String, String, usize)>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Map ID cannot be empty"));
    }

    let theme: Theme = theme_str
        .parse::<Theme>()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let bytes = state.storage.get_curated_layer_image(&theme, &id, layer_index)?;

    let filename = state
        .storage
        .list_curated(Some(&theme))
        .ok()
        .and_then(|list| {
            list.into_iter().find(|m| m.id == id).and_then(|m| {
                m.layers
                    .into_iter()
                    .find(|l| l.index == layer_index)
                    .map(|l| l.filename)
            })
        });

    let mime = detect_image_mime(&bytes, filename.as_deref());

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(response)
}


/// Request payload for curating a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateRequest {
    pub id: String,
    pub theme: Theme,
    pub rating: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub layers: Option<Vec<CandidateLayer>>,
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

/// POST /api/curate - Move candidate to curated theme directory with rating and tags.
pub async fn post_curate(
    State(state): State<AppState>,
    Json(payload): Json<CurateRequest>,
) -> Result<Json<CurateResponse>, ApiError> {
    if payload.id.trim().is_empty() {
        return Err(ApiError::bad_request("Candidate ID cannot be empty"));
    }

    if !CuratedMap::is_valid_rating(payload.rating) {
        return Err(ApiError::bad_request(format!(
            "Rating must be between 1 and 5, got {}",
            payload.rating
        )));
    }

    // Inspect candidate in staging to preserve dimensions and validate layers
    let candidates = state.storage.list_staging()?;
    let candidate = candidates
        .iter()
        .find(|c| c.id == payload.id)
        .ok_or_else(|| ApiError::not_found(format!("Candidate '{}' not found in staging", payload.id)))?;

    let dimensions = candidate.dimensions;

    // Validate layers
    let layers_to_check: Vec<CandidateLayer> = if let Some(req_layers) = &payload.layers {
        req_layers.clone()
    } else {
        candidate.resolved_layers()
    };

    if let Some(unassigned_layer) = layers_to_check.iter().find(|l| l.role.is_unassigned()) {
        return Err(ApiError::bad_request(format!(
            "Cannot curate bundle: layer {} is Unassigned",
            unassigned_layer.index
        )));
    }

    let included_layers: Vec<&CandidateLayer> = layers_to_check
        .iter()
        .filter(|l| l.role.is_included())
        .collect();

    if included_layers.is_empty() {
        return Err(ApiError::bad_request("At least 1 layer must be included"));
    }

    // If request provided layers, update them in staging before curation
    if let Some(layers) = payload.layers {
        state.storage.update_staging_layers(&payload.id, layers)?;
    }

    let curated = CuratedMap::with_now(
        &payload.id,
        payload.theme,
        payload.rating,
        payload.tags,
        dimensions,
    );

    state.storage.curate_staging(&curated)?;

    // Read back curated map with populated layers if available
    let curated_result = state
        .storage
        .list_curated(Some(&payload.theme))
        .ok()
        .and_then(|list| list.into_iter().find(|m| m.id == payload.id))
        .unwrap_or(curated);

    tracing::info!(
        id = %payload.id,
        theme = %payload.theme,
        "Curated candidate image"
    );

    Ok(Json(CurateResponse {
        status: "ok",
        curated: curated_result,
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

    let updated_candidate = state.storage.update_staging_layers(&id, payload.layers)?;

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

    let split_candidates = state.storage.split_staging_bundle(&id)?;

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

    let regrouped_candidate = state.storage.regroup_staging_bundle(&parent_id)?;

    Ok(Json(regrouped_candidate))
}

/// Request payload for rejecting a candidate.
#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    pub id: String,
    pub reason: RejectionReason,
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

    state
        .storage
        .reject_staging(&payload.id, payload.reason, payload.notes.as_deref())?;

    tracing::info!(
        id = %payload.id,
        reason = ?payload.reason,
        "Rejected candidate image"
    );

    Ok(Json(RejectResponse {
        status: "ok",
        id: payload.id,
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
    let service = RedditIngestionService::new(state.harvester.clone(), state.storage.clone());

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

/// Request payload for initiating a batch AI benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunRequest {
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub archetypes: Vec<String>,
    #[serde(default)]
    pub custom_prompts: Vec<CustomPrompt>,
    #[serde(default = "default_true")]
    pub use_reference_conditioning: bool,
}

fn default_true() -> bool {
    true
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

    let service = state
        .get_or_create_benchmark_service()?
        .as_ref()
        .clone()
        .with_reference_conditioning(payload.use_reference_conditioning);

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
