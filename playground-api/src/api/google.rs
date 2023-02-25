use crate::{
  env_var,
  http::{json_response, JsonResult},
  GracefulExit,
};

use super::{APIError, APIResult};

use std::{fmt::Display, str::FromStr};

use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer, Serialize};

use format as f;

const DRIVE_API: &str = "https://www.googleapis.com/drive/v3";
const DRIVE_FILE_FIELDS: &str = "name,size,videoMediaMetadata,mimeType";

static API_KEY: Lazy<String> = Lazy::new(|| {
  env_var("GOOGLE_API_KEY").unwrap_or_exit("Could not initialize google API")
});

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveVideoMetadata {
  pub width: u16,
  pub height: u16,
  #[serde(deserialize_with = "deserialize_number_from_string")]
  pub duration_millis: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
  pub mime_type: String,
  pub name: String,
  #[serde(
    alias = "size",
    deserialize_with = "deserialize_option_number_from_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub size_bytes: Option<u64>,
  #[serde(
    alias = "videoMediaMetadata",
    skip_serializing_if = "Option::is_none"
  )]
  pub video_metadata: Option<DriveVideoMetadata>,
}

pub fn thumbnail_url(video_id: &str) -> String {
  f!("https://drive.google.com/thumbnail?id={video_id}")
}

pub async fn get_file(
  file_id: &str,
  request_client: &reqwest::Client,
) -> APIResult<DriveFile> {
  let response = request_client
    .get(&f!(
      "{DRIVE_API}/files/{file_id}?fields={DRIVE_FILE_FIELDS}&trashed=false&key={}",
      *API_KEY
    ))
    .send()
    .await?;

  match json_response(response).await? {
    JsonResult::Typed(file) => Ok(file),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file)),
  }
}

pub fn deserialize_option_number_from_string<'de, T, D>(
  deserializer: D,
) -> Result<Option<T>, D::Error>
where
  D: Deserializer<'de>,
  T: FromStr + serde::Deserialize<'de>,
  <T as FromStr>::Err: Display,
{
  #[derive(Deserialize)]
  #[serde(untagged)]
  enum NumericOrNull<'a, T> {
    Str(&'a str),
    FromStr(T),
    Null,
  }

  match NumericOrNull::<T>::deserialize(deserializer)? {
    NumericOrNull::Str(s) => match s {
      "" => Ok(None),
      _ => T::from_str(s).map(Some).map_err(serde::de::Error::custom),
    },
    NumericOrNull::FromStr(i) => Ok(Some(i)),
    NumericOrNull::Null => Ok(None),
  }
}

pub fn deserialize_number_from_string<'de, T, D>(
  deserializer: D,
) -> Result<T, D::Error>
where
  D: Deserializer<'de>,
  T: FromStr + serde::Deserialize<'de>,
  <T as FromStr>::Err: Display,
{
  #[derive(Deserialize)]
  #[serde(untagged)]
  enum StringOrInt<T> {
    String(String),
    Number(T),
  }

  match StringOrInt::<T>::deserialize(deserializer)? {
    StringOrInt::String(s) => s.parse::<T>().map_err(serde::de::Error::custom),
    StringOrInt::Number(i) => Ok(i),
  }
}
