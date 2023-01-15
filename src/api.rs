use thiserror::Error;

use axum::{
  response::{IntoResponse, Response},
  Json,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Error, Debug)]
pub enum APIError {
  #[error("JSON structure did not match type {0:?}")]
  JsonParsing(serde_json::Value),
  #[error("Status code error {0}: {1:?}")]
  StatusCode(StatusCode, Option<serde_json::Value>),
  #[error("Unauthorized: {0}")]
  Unauthorized(String),
}

pub struct APIResponseError(anyhow::Error);

impl IntoResponse for APIResponseError {
  fn into_response(self) -> Response {
    (
      StatusCode::INTERNAL_SERVER_ERROR,
      Json(APIErrorResponse {
        error: self.0.to_string(),
      }),
    )
      .into_response()
  }
}

impl<E> From<E> for APIResponseError
where
  E: Into<anyhow::Error>,
{
  fn from(err: E) -> Self {
    Self(err.into())
  }
}

pub type APIResponseResult<T> = Result<T, APIResponseError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct APIErrorResponse {
  error: String,
}
