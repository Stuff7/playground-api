pub mod google;

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
  #[error("External Request failed: {0}")]
  ExternalRequest(#[from] reqwest::Error),
  #[error("Could not parse header value into a str: {0}")]
  HeaderValueParsing(#[from] reqwest::header::ToStrError),
  #[error("Invalid JSON: {0}")]
  InvalidJson(#[from] serde_json::Error),
  #[error("Bad query: {0}")]
  BadQuery(#[from] axum::extract::rejection::QueryRejection),
  #[error("Bad path: {0}")]
  BadPath(#[from] axum::extract::rejection::PathRejection),
  #[error("Bad JSON: {0}")]
  BadJson(#[from] axum::extract::rejection::JsonRejection),
  #[error("JSON structure did not match type")]
  JsonParsing(serde_json::Value),
  #[error("Failed to parse header value: {0}")]
  HeaderParsing(#[from] InvalidHeaderValue),
  #[error("External request returned bad status code {0}")]
  StatusCode(StatusCode, Option<serde_json::Value>),
  #[error("Bad Request: {0}")]
  BadRequest(String),
  #[error("Internal Server Error: {0}")]
  Internal(String),
  #[error("Unauthorized: {0}")]
  UnauthorizedMessage(String),
  #[error("Unauthorized")]
  Unauthorized,
  #[error("JWT Error: {0}")]
  Jwt(#[from] JWTError),
  #[error("OAuth Error: {0}")]
  OAuth(#[from] OAuthError),
  #[error("Database Error: {0}")]
  Database(#[from] DBError),
  #[error("{0}")]
  Conflict(String),
  #[error("{0}")]
  NotFound(String),
}

impl IntoResponse for APIError {
  fn into_response(self) -> Response {
    let (status, body) = match self {
      Self::NotFound(_) => (StatusCode::NOT_FOUND, None),
      Self::Conflict(_) => (StatusCode::CONFLICT, None),
      Self::BadRequest(_) | Self::BadQuery(_) | Self::BadPath(_) | Self::BadJson(_) => {
        (StatusCode::BAD_REQUEST, None)
      }
      Self::JsonParsing(ref data) => (StatusCode::NOT_ACCEPTABLE, Some(data.clone())),
      Self::InvalidJson(_) => (StatusCode::NOT_ACCEPTABLE, None),
      Self::StatusCode(ref code, ref data) => (*code, data.clone()),
      Self::Jwt(_) | Self::Unauthorized | Self::UnauthorizedMessage(_) | Self::OAuth(_) => {
        (StatusCode::UNAUTHORIZED, None)
      }
      Self::HeaderParsing(_)
      | Self::Internal(_)
      | Self::Database(_)
      | Self::HeaderValueParsing(_) => (StatusCode::INTERNAL_SERVER_ERROR, None),
      Self::ExternalRequest(ref request) => (
        request
          .status()
          .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        None,
      ),
    };
    (
      status,
      Json(APIErrorBody {
        status_code: status.as_u16(),
        error: status.to_string(),
        message: self.to_string(),
        details: body,
      }),
    )
      .into_response()
  }
}

pub type APIResult<T = ()> = Result<T, APIError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct APIErrorResponse {
  error: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct APIErrorBody {
  status_code: u16,
  error: String,
  message: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  details: Option<serde_json::Value>,
}
