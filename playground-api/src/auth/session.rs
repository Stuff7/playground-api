use std::collections::{HashMap, HashSet};

use crate::{
  api::{APIError, APIResult},
  db::{
    files::{File, FileMetadata, PartialFile},
    users::User,
    Database,
  },
  string::NonEmptyString,
  GracefulExit,
};

use axum::{
  async_trait,
  extract::{FromRequestParts, Path, Query, TypedHeader},
  headers::{authorization::Bearer, Authorization},
  http::request::Parts,
  RequestPartsExt,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::jwt;

use format as f;

pub static SESSIONS_CACHE: Lazy<Mutex<HashSet<String>>> =
  Lazy::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionCache {
  _id: String,
  pub sessions: HashSet<String>,
}

#[derive(Debug, Serialize)]
pub struct Session {
  pub user_id: String,
}

impl Session {
  pub async fn get_user(&self, database: &Database) -> APIResult<User> {
    database
      .find_by_id::<User>(self.user_id.as_ref())
      .await?
      .ok_or(APIError::Unauthorized)
  }

  pub async fn save(token: &str) {
    SESSIONS_CACHE.lock().await.insert(token.to_string());
  }

  pub async fn invalidate(token: &str) {
    SESSIONS_CACHE.lock().await.remove(token);
  }

  pub async fn from_token(token: &str) -> APIResult<Self> {
    let mut cache = SESSIONS_CACHE.lock().await;
    let user_id = cache
      .contains(token)
      .then(|| jwt::verify_token(token).map(|token| token.claims.sub))
      .ok_or_else(|| {
        APIError::UnauthorizedMessage("Invalid session".to_string())
      })?
      .map_err(|err| {
        cache.remove(token);
        APIError::from(err)
      })?;
    Ok(Self { user_id })
  }
}

#[async_trait]
impl<S> FromRequestParts<S> for Session
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(
    parts: &mut Parts,
    _: &S,
  ) -> Result<Self, Self::Rejection> {
    let bearer: Option<TypedHeader<Authorization<Bearer>>> = parts
      .extract()
      .await
      .unwrap_or_exit("Could not extract Authorization header");

    let token =
      bearer
        .map(|bearer| bearer.token().to_string())
        .ok_or_else(|| {
          APIError::UnauthorizedMessage(
            "Missing/Invalid Authorization header".to_string(),
          )
        })?;

    Ok(Self::from_token(&token).await?)
  }
}

pub struct SessionQuery(pub Session);

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenQuery {
  pub token: String,
}

#[async_trait]
impl<S> FromRequestParts<S> for SessionQuery
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(
    parts: &mut Parts,
    _: &S,
  ) -> Result<Self, Self::Rejection> {
    let Query(query) = parts.extract::<Query<TokenQuery>>().await?;
    Ok(Self(Session::from_token(&query.token).await?))
  }
}

#[async_trait]
impl<S> FromRequestParts<S> for PartialFile
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(
    parts: &mut Parts,
    _: &S,
  ) -> Result<Self, Self::Rejection> {
    let session = parts.extract::<Session>().await?;
    let Query(query) =
      parts.extract::<Query<HashMap<String, String>>>().await?;

    Ok(Self {
      id: query.get(Self::id()).cloned(),
      folder_id: query.get(Self::folder_id()).cloned(),
      user_id: Some(session.user_id),
      name: query
        .get(Self::name())
        .map(NonEmptyString::try_from)
        .transpose()?,
      metadata: query.get("type").and_then(|t| {
        if t == "folder" {
          Some(FileMetadata::Folder)
        } else {
          None
        }
      }),
    })
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileIdVec {
  pub id: String,
}

pub struct FileIdVecQuery(pub HashSet<String>);

#[async_trait]
impl<S> FromRequestParts<S> for FileIdVecQuery
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(
    parts: &mut Parts,
    _: &S,
  ) -> Result<Self, Self::Rejection> {
    let Query(query) = parts.extract::<Query<FileIdVec>>().await?;
    Ok(Self(query.id.split(',').map(String::from).collect()))
  }
}

#[derive(Deserialize)]
pub struct FileIdPath {
  pub file_id: String,
}

pub struct FileId(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for FileId
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(
    parts: &mut Parts,
    _: &S,
  ) -> Result<Self, Self::Rejection> {
    let Path(FileIdPath { file_id }) =
      parts.extract::<Path<FileIdPath>>().await?;
    Ok(Self(file_id))
  }
}

/// Asserts:
/// * File with `folder_id` exists in files collection
/// * File with `folder_id` is a folder
/// * File belongs to user with `user_id`
///
/// Returns the `folder_id`, if root alias is used it returns the root folder_id
pub async fn assert_valid_folder(
  user_id: &str,
  folder_id: &Option<String>,
  database: &Database,
) -> APIResult<Option<String>> {
  let mut result = folder_id.clone();
  if let Some(folder_id) = folder_id.as_deref() {
    let folder_id = File::map_folder_id(user_id, folder_id);
    let folder =
      database
        .find_by_id::<File>(folder_id)
        .await?
        .ok_or_else(|| {
          APIError::BadRequest(f!(
            "Folder with id {folder_id:?} does not exist"
          ))
        })?;

    if !matches!(folder.metadata, FileMetadata::Folder) {
      return Err(APIError::BadRequest(f!(
        "File with id {folder_id:?} is not a folder"
      )));
    }

    if folder.user_id != user_id {
      return Err(APIError::Unauthorized);
    }
    result = Some(folder_id.to_string());
  }
  Ok(result)
}
