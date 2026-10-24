use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::ports::storage_error::StorageError;

/// API error types converted into appropriate HTTP status codes and JSON payloads.
#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(String),
    BadGateway(String),
    ServiceUnavailable(String),
    DatasetNotReady {
        error: String,
        blocking_issues: Vec<crate::domain::BlockingIssue>,
        readiness: Box<crate::domain::DatasetReadiness>,
    },
    SnapshotInUse {
        id: String,
    },
    RunAlreadyActive {
        active_run_id: String,
    },
    ModalNotConfigured(String),
    ModelNotReferenceCapable {
        model: String,
    },
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

    pub fn model_not_reference_capable(model: impl Into<String>) -> Self {
        Self::ModelNotReferenceCapable {
            model: model.into(),
        }
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

impl From<crate::services::FreezeDatasetError> for ApiError {
    fn from(err: crate::services::FreezeDatasetError) -> Self {
        match err {
            crate::services::FreezeDatasetError::NotReady {
                summary,
                blocking_issues,
                readiness,
            } => ApiError::DatasetNotReady {
                error: summary,
                blocking_issues,
                readiness: Box::new(readiness),
            },
            crate::services::FreezeDatasetError::Storage(storage_err) => storage_err.into(),
        }
    }
}

impl From<crate::services::DeleteDatasetError> for ApiError {
    fn from(err: crate::services::DeleteDatasetError) -> Self {
        match err {
            crate::services::DeleteDatasetError::Referenced(id) => ApiError::SnapshotInUse { id },
            crate::services::DeleteDatasetError::Storage(storage_err) => storage_err.into(),
        }
    }
}
impl From<crate::services::TrainingServiceError> for ApiError {
    fn from(err: crate::services::TrainingServiceError) -> Self {
        match err {
            crate::services::TrainingServiceError::ModalNotConfigured(msg) => {
                ApiError::ModalNotConfigured(msg)
            }
            crate::services::TrainingServiceError::RunAlreadyActive { active_run_id } => {
                ApiError::RunAlreadyActive { active_run_id }
            }
            crate::services::TrainingServiceError::RunNotFound(id) => {
                ApiError::NotFound(format!("Training run '{id}' not found"))
            }
            crate::services::TrainingServiceError::AdapterNotFound(id) => {
                ApiError::NotFound(format!("LoRA adapter '{id}' not found"))
            }
            crate::services::TrainingServiceError::SnapshotNotFound(id) => {
                ApiError::NotFound(format!("Dataset snapshot '{id}' not found"))
            }
            crate::services::TrainingServiceError::SnapshotBlobMissing(id) => {
                ApiError::BadRequest(format!("Dataset snapshot '{id}' ZIP archive is missing from storage"))
            }
            crate::services::TrainingServiceError::InvalidState(msg) => ApiError::BadRequest(msg),
            crate::services::TrainingServiceError::Conflict(msg) => ApiError::Conflict(msg),
            crate::services::TrainingServiceError::Storage(err) => err.into(),
            crate::services::TrainingServiceError::Runner(crate::ports::TrainingRunnerError::NotConfigured(msg)) => {
                ApiError::ModalNotConfigured(msg)
            }
            crate::services::TrainingServiceError::Runner(err) => ApiError::BadGateway(err.to_string()),
            crate::services::TrainingServiceError::Cost(err) => ApiError::Internal(err.to_string()),
        }
    }
}


impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::DatasetNotReady {
                error,
                blocking_issues,
                readiness,
            } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "code": "DATASET_NOT_READY",
                    "error": error,
                    "blocking_issues": blocking_issues,
                    "readiness": *readiness,
                })),
            )
                .into_response(),
            ApiError::SnapshotInUse { id } => (
                StatusCode::CONFLICT,
                Json(json!({
                    "code": "SNAPSHOT_IN_USE",
                    "error": format!(
                        "Dataset snapshot '{id}' is referenced by one or more training runs and cannot be deleted"
                    ),
                })),
            )
                .into_response(),
            ApiError::RunAlreadyActive { active_run_id } => (
                StatusCode::CONFLICT,
                Json(json!({
                    "code": "RUN_ALREADY_ACTIVE",
                    "error": format!("Cannot dispatch training run: another run '{active_run_id}' is currently running"),
                    "active_run_id": active_run_id,
                })),
            )
                .into_response(),
            ApiError::ModalNotConfigured(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "code": "MODAL_NOT_CONFIGURED",
                    "error": msg,
                })),
            )
                .into_response(),
            ApiError::ModelNotReferenceCapable { model } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "code": "MODEL_NOT_REFERENCE_CAPABLE",
                    "error": format!("Model '{model}' does not support reference conditioning"),
                    "model": model,
                })),
            )
                .into_response(),
            ApiError::NotFound(msg) => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::Conflict(msg) => {
                (StatusCode::CONFLICT, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::Internal(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::BadGateway(msg) => {
                (StatusCode::BAD_GATEWAY, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::ServiceUnavailable(msg) => {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": msg }))).into_response()
            }
        }
    }
}
