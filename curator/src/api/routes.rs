use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::Response,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::domain::{Candidate, CuratedMap, RejectionReason, Theme};

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

/// GET /api/image/curated/{theme}/{id} - Stream binary image for a curated map.
pub async fn get_curated_image(
    State(state): State<AppState>,
    Path((theme_str, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("Map ID cannot be empty"));
    }

    let theme: Theme = theme_str
        .parse()
        .map_err(|e| ApiError::bad_request(format!("{e}")))?;

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

/// Request payload for curating a candidate.
#[derive(Debug, Deserialize)]
pub struct CurateRequest {
    pub id: String,
    pub theme: Theme,
    pub rating: u8,
    #[serde(default)]
    pub tags: Vec<String>,
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

    // Inspect candidate in staging to preserve dimensions
    let candidates = state.storage.list_staging()?;
    let candidate = candidates.iter().find(|c| c.id == payload.id);
    let dimensions = candidate.map(|c| c.dimensions).unwrap_or((0, 0));

    let curated = CuratedMap::with_now(
        &payload.id,
        payload.theme,
        payload.rating,
        payload.tags,
        dimensions,
    );

    state.storage.curate_staging(&curated)?;

    Ok(Json(CurateResponse {
        status: "ok",
        curated,
    }))
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

    Ok(Json(RejectResponse {
        status: "ok",
        id: payload.id,
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
