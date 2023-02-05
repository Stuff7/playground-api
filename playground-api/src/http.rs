use axum::{
  http::{HeaderMap, HeaderValue},
  response::IntoResponse,
};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::api::{APIError, APIResult};

use format as f;

const CONTENT_LENGTH: usize = 2 * 1024 * 1000; // 2MB
const FIRST_CONTENT_LENGTH: usize = 5 * 1024 * 1000; // 5MB

pub fn get_range(headers: HeaderMap) -> (usize, usize) {
  let raw_range = match headers.get("Range") {
    Some(header) => header
      .to_str()
      .unwrap_or_default()
      .get(6..)
      .unwrap_or_default()
      .split('-')
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

pub enum JsonResult<T: DeserializeOwned> {
  Typed(T),
  Untyped(serde_json::Value),
}

pub async fn json_response<T: serde::de::DeserializeOwned>(
  response: reqwest::Response,
) -> APIResult<JsonResult<T>> {
  let status_code = response.status();

  if status_code.is_client_error() || status_code.is_server_error() {
    return Err(APIError::StatusCode(
      status_code,
      response.json::<serde_json::Value>().await.ok(),
    ));
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

pub fn extract_header(headers: &HeaderMap, key: &str) -> APIResult<HeaderValue> {
  Ok(
    headers
      .get(key)
      .ok_or_else(|| APIError::Internal(f!("No {key:?} header found")))?
      .into(),
  )
}

/// Download video and stream on demand.
pub async fn stream_video(
  video_url: &str,
  headers: HeaderMap,
  client: &reqwest::Client,
) -> APIResult<impl IntoResponse> {
  let (range_start, range_end) = get_range(headers);
  let byte_range = f!("{range_start}-{range_end}");

  let response = client
    .get(video_url)
    .header("Range", f!("bytes={byte_range}"))
    .send()
    .await?;

  let headers = response.headers();
  let content_range = extract_header(headers, "Content-Range")?;
  let content_type = extract_header(headers, "Content-Type")?;

  let body = response.bytes().await?;

  let mut headers = HeaderMap::new();
  headers.insert("Accept-Ranges", "bytes".parse()?);
  headers.insert("Content-Range", content_range);
  headers.insert("Content-Type", content_type);

  Ok((StatusCode::PARTIAL_CONTENT, headers, body))
}
