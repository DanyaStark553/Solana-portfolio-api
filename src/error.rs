use crate::models::ErrorResponse;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum AppError {
    InvalidWallet(String),
    InvalidPositionData(String),
    UpstreamTimeout,
    UpstreamError(String),
    Storage(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWallet(msg) => write!(f, "Invalid wallet: {msg}"),
            Self::InvalidPositionData(msg) => write!(f, "Invalid position data: {msg}"),
            Self::UpstreamTimeout => write!(f, "Upstream request timed out"),
            Self::UpstreamError(msg) => write!(f, "Upstream error: {msg}"),
            Self::Storage(msg) => write!(f, "Internal storage error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::InvalidWallet(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::InvalidPositionData(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Invalid position data: {msg}"),
            ),
            Self::UpstreamTimeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "Upstream service timed out".to_string(),
            ),
            Self::UpstreamError(msg) => (
                StatusCode::BAD_GATEWAY,
                format!("Upstream service failed: {msg}"),
            ),
            Self::Storage(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Storage error: {msg}"),
            ),
        };

        (status, Json(ErrorResponse { error: message })).into_response()
    }
}
