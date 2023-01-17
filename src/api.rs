use thiserror::Error;

use axum::{
  response::{IntoResponse, Response},
  Json,
};
use reqwest::{header::InvalidHeaderValue, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
  auth::{jwt::JWTError, oauth::OAuthError},
  db::DBError,
};

#[derive(Error, Debug)]
pub enum APIError {
  #[error(transparent)]
  ExternalRequest(#[from] reqwest::Error),
  #[error(transparent)]
  InvalidJson(#[from] serde_json::Error),
  #[error("JSON structure did not match type {0:?}")]
  JsonParsing(serde_json::Value),
  #[error(transparent)]
  HeaderParsing(#[from] InvalidHeaderValue),
  #[error("External request returned bad status code {0}: {1:?}")]
  StatusCode(StatusCode, Option<serde_json::Value>),
  #[error("Internal Server Error: {0}")]
  Internal(String),
  #[error("Unauthorized: {0}")]
  Unauthorized(String),
  #[error(transparent)]
  JWT(#[from] JWTError),
  #[error(transparent)]
  OAuth(#[from] OAuthError),
  #[error(transparent)]
  Database(#[from] DBError),
}

impl IntoResponse for APIError {
  fn into_response(self) -> Response {
    let status = match self {
      Self::InvalidJson(_) | Self::JsonParsing(_) => StatusCode::NOT_ACCEPTABLE,
      Self::StatusCode(code, _) => code,
      Self::JWT(_) | Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
      Self::HeaderParsing(_) | Self::Internal(_) | Self::Database(_) => {
        StatusCode::INTERNAL_SERVER_ERROR
      }
      Self::ExternalRequest(ref request) => request
        .status()
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
      Self::OAuth(ref err) => match err {
        OAuthError::BadStatus(status) => status.clone(),
        _ => StatusCode::UNAUTHORIZED,
      },
    };
    (
      status,
      Json(APIErrorBody {
        status_code: status.as_u16(),
        error: status.to_string(),
        message: self.to_string(),
      }),
    )
      .into_response()
  }
}

pub type APIResult<T> = Result<T, APIError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct APIErrorResponse {
  error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct APIErrorBody {
  status_code: u16,
  error: String,
  message: String,
}
