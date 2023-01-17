use axum::http::HeaderMap;
use serde::de::DeserializeOwned;

use crate::api::{APIError, APIResult};

const CONTENT_LENGTH: usize = 2 * 1024 * 1000; // 2MB
const FIRST_CONTENT_LENGTH: usize = 5 * 1024 * 1000; // 5MB

pub fn get_range(headers: HeaderMap) -> (usize, usize) {
  let raw_range = match headers.get("Range") {
    Some(header) => header
      .to_str()
      .unwrap_or_default()
      .get(6..)
      .unwrap_or_default()
      .split("-")
      .map(|v| v.parse::<usize>().ok())
      .collect::<Vec<_>>(),
    None => vec![Some(0), Some(FIRST_CONTENT_LENGTH)],
  };

  let start = raw_range
    .get(0)
    .copied()
    .unwrap_or_default()
    .unwrap_or_default();

  let end = raw_range.get(1).copied().unwrap_or_default().unwrap_or(
    start
      + if start == 0 {
        FIRST_CONTENT_LENGTH
      } else {
        CONTENT_LENGTH
      },
  );

  (start, end)
}

pub async fn json_response<T: serde::de::DeserializeOwned>(
  response: reqwest::Response,
) -> APIResult<JsonResult<T>> {
  let status_code = response.status();

  if status_code.is_client_error() || status_code.is_server_error() {
    return Err(
      APIError::StatusCode(status_code, response.json::<serde_json::Value>().await.ok()).into(),
    );
  }

  let response_text = response
    .text()
    .await
    .map_err(|_| APIError::Internal("Response has no body".into()))?;
  let typed = serde_json::from_str::<T>(&response_text);
  match typed {
    Ok(file) => Ok(JsonResult::Typed(file)),
    Err(_) => Ok(JsonResult::Untyped(serde_json::from_str::<
      serde_json::Value,
    >(&response_text)?)),
  }
}

pub enum JsonResult<T: DeserializeOwned> {
  Typed(T),
  Untyped(serde_json::Value),
}
