//! Error handling for Atomic API following AGENTS.md error handling strategy
//!
//! Implements a focused error hierarchy using `thiserror` for Atomic VCS API operations
//! with automatic error conversion and context-rich error messages.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result type alias for API operations
pub type ApiResult<T> = Result<T, ApiError>;

/// Main error type for the Atomic API following AGENTS.md hierarchical error patterns
#[derive(Debug, Error)]
pub enum ApiError {
    /// Repository errors - wrapping underlying VCS errors
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Database errors from underlying Sanakirja
    #[error("Database error: {message}")]
    Database { message: String },

    /// Internal server errors
    #[error("Internal server error: {message}")]
    Internal { message: String },
}

/// Repository-specific errors following AGENTS.md error conversion patterns
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum RepositoryError {
    #[error("Repository not found at path: {path}")]
    NotFound { path: String },

    #[error("Repository access denied: {path}")]
    AccessDenied { path: String },

    #[error("Repository is corrupted: {path}")]
    Corrupted { path: String },

    #[error("Channel '{channel}' not found in repository")]
    ChannelNotFound { channel: String },

    #[error("Change '{change_id}' not found")]
    ChangeNotFound { change_id: String },

    #[error("File '{file_path}' not found")]
    FileNotFound { file_path: String },
}

/// Error response format for JSON API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    pub code: String,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(error_type: &str, message: String, code: String) -> Self {
        Self {
            error: error_type.to_string(),
            message,
            code,
        }
    }
}

/// Convert ApiError to HTTP responses following AGENTS.md error handling patterns
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, message, code) = match &self {
            ApiError::Repository(err) => match err {
                RepositoryError::NotFound { .. } => (
                    StatusCode::NOT_FOUND,
                    "repository_not_found",
                    err.to_string(),
                    "REPO_001".to_string(),
                ),
                RepositoryError::AccessDenied { .. } => (
                    StatusCode::FORBIDDEN,
                    "repository_access_denied",
                    err.to_string(),
                    "REPO_002".to_string(),
                ),
                RepositoryError::ChannelNotFound { .. } => (
                    StatusCode::NOT_FOUND,
                    "channel_not_found",
                    err.to_string(),
                    "REPO_003".to_string(),
                ),
                RepositoryError::ChangeNotFound { .. } => (
                    StatusCode::NOT_FOUND,
                    "change_not_found",
                    err.to_string(),
                    "REPO_004".to_string(),
                ),
                RepositoryError::FileNotFound { .. } => (
                    StatusCode::NOT_FOUND,
                    "file_not_found",
                    err.to_string(),
                    "REPO_005".to_string(),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "repository_error",
                    err.to_string(),
                    "REPO_999".to_string(),
                ),
            },
            ApiError::Io(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "io_error",
                "Internal I/O error occurred".to_string(),
                "IO_001".to_string(),
            ),
            ApiError::Database { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Database operation failed".to_string(),
                "DB_001".to_string(),
            ),
            ApiError::Internal { message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                message.clone(),
                "INT_001".to_string(),
            ),
        };

        let error_response = ErrorResponse::new(error_type, message, code);
        (status, Json(error_response)).into_response()
    }
}

/// Automatic error conversion from anyhow errors following AGENTS.md patterns
impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::Database {
            message: err.to_string(),
        }
    }
}

/// Helper functions for common error patterns
impl ApiError {
    /// Create a repository not found error
    pub fn repository_not_found(path: impl Into<String>) -> Self {
        ApiError::Repository(RepositoryError::NotFound { path: path.into() })
    }

    /// Create an internal error with context
    pub fn internal(message: impl Into<String>) -> Self {
        ApiError::Internal {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = ApiError::repository_not_found("/invalid/path");
        assert!(matches!(
            err,
            ApiError::Repository(RepositoryError::NotFound { .. })
        ));
    }

    #[test]
    fn test_error_response_creation() {
        let response = ErrorResponse::new(
            "test_error",
            "Test message".to_string(),
            "TEST_001".to_string(),
        );
        assert_eq!(response.error, "test_error");
        assert_eq!(response.message, "Test message");
        assert_eq!(response.code, "TEST_001");
    }

    #[test]
    fn test_error_conversion() {
        let repo_err = RepositoryError::NotFound {
            path: "/test".to_string(),
        };
        let api_err = ApiError::Repository(repo_err);

        // Test that the error can be converted to a response
        let _response = api_err.into_response();
    }
}
