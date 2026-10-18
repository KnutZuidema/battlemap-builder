use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::ports::storage::StorageError;

/// API error types converted into appropriate HTTP status codes and JSON payloads.
#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(String),
    BadGateway(String),
    ServiceUnavailable(String),
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn bad_gateway(msg: impl Into<String>) -> Self {
        Self::BadGateway(msg.into())
    }

    pub fn service_unavailable(msg: impl Into<String>) -> Self {
        Self::ServiceUnavailable(msg.into())
    }
}

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound(msg) => ApiError::NotFound(msg),
            StorageError::AlreadyExists(msg) => ApiError::Conflict(msg),
            StorageError::InvalidData(msg) => ApiError::BadRequest(msg),
            StorageError::Io(err) => ApiError::Internal(err.to_string()),
            StorageError::Serialization(err) => ApiError::Internal(err.to_string()),
            StorageError::Zip(err) => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<crate::services::IngestionError> for ApiError {
    fn from(err: crate::services::IngestionError) -> Self {
        match err {
            crate::services::IngestionError::Storage(storage_err) => storage_err.into(),
            crate::services::IngestionError::Harvester(harvester_err) => {
                ApiError::Internal(format!("Harvester error: {harvester_err}"))
            }
            crate::services::IngestionError::Http(http_err) => {
                ApiError::Internal(format!("HTTP error: {http_err}"))
            }
            crate::services::IngestionError::Serialization(ser_err) => {
                ApiError::Internal(format!("Serialization error: {ser_err}"))
            }
            crate::services::IngestionError::Io(io_err) => {
                ApiError::Internal(format!("I/O error: {io_err}"))
            }
        }
    }
}

impl From<crate::services::description::DescriptionError> for ApiError {
    fn from(err: crate::services::description::DescriptionError) -> Self {
        use crate::services::description::DescriptionError;
        match err {
            DescriptionError::NotConfigured(msg) => ApiError::ServiceUnavailable(msg),
            DescriptionError::ImageUnavailable(msg) => ApiError::NotFound(msg),
            DescriptionError::GenerationFailed(msg) => ApiError::BadGateway(msg),
            DescriptionError::EmptyDescription => {
                ApiError::BadGateway("Empty description received from generator".into())
            }
        }
    }
}
impl From<crate::services::curation::CurationWorkflowError> for ApiError {
    fn from(err: crate::services::curation::CurationWorkflowError) -> Self {
        use crate::services::curation::CurationWorkflowError;
        match err {
            CurationWorkflowError::ValidationFailed(msg) => ApiError::BadRequest(msg),
            CurationWorkflowError::NotFound(msg) => ApiError::NotFound(msg),
            CurationWorkflowError::DescriptionFailed(desc_err) => desc_err.into(),
            CurationWorkflowError::Storage(msg) => ApiError::Internal(msg),
        }
    }
}


impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::BadGateway(msg) => (StatusCode::BAD_GATEWAY, msg),
            ApiError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
