use std::collections::{HashMap, HashSet};

use crate::{
  api::{APIError, APIResult},
  db, GracefulExit,
};

use axum::{
  async_trait,
  body::Body,
  extract::{FromRequest, FromRequestParts, Path, Query, TypedHeader},
  headers::{authorization::Bearer, Authorization},
  http::{request::Parts, Request},
  Json, RequestExt, RequestPartsExt,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::jwt;

use format as f;

#[derive(Debug, Serialize)]
pub struct Session {
  pub user_id: String,
}

impl Session {
  pub async fn get_user(&self) -> APIResult<db::User> {
    db::DATABASE
      .find_by_id::<db::User>(self.user_id.as_ref())
      .await?
      .ok_or(APIError::Unauthorized)
  }

  pub async fn save(token: &str) {
    db::SESSIONS_CACHE.lock().await.insert(token.to_string());
  }

  pub async fn invalidate(token: &str) {
    db::SESSIONS_CACHE.lock().await.remove(token);
  }

  pub async fn from_token(token: &str) -> APIResult<Self> {
    let mut cache = db::SESSIONS_CACHE.lock().await;
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
impl<S> FromRequestParts<S> for db::PartialUserFile
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
      folder_id: assert_valid_folder(
        &session.user_id,
        &query.get(Self::folder_id()).cloned(),
      )
      .await?,
      user_id: Some(session.user_id),
      name: query
        .get(Self::name())
        .map(db::NonEmptyString::try_from)
        .transpose()?,
      metadata: query.get("type").and_then(|t| {
        if t == "folder" {
          Some(db::FileMetadata::Folder)
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
    // TODO: error handling
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

#[derive(Serialize)]
pub struct FolderBody<T: DeserializeOwned>(pub Option<String>, pub T);

#[async_trait]
impl<S, T> FromRequest<S, Body> for FolderBody<T>
where
  S: Send + Sync,
  T: DeserializeOwned,
{
  type Rejection = APIError;

  async fn from_request(
    mut req: Request<Body>,
    _: &S,
  ) -> Result<Self, Self::Rejection> {
    let session = req.extract_parts::<Session>().await?;
    let file_id = req.extract_parts::<FileId>().await;
    let Json(body) = req.extract::<Json<serde_json::Value>, _>().await?;
    let folder = body
      .get("folder")
      .and_then(|v| v.as_str())
      .map(String::from);

    if let Ok(FileId(file_id)) = file_id {
      assert_writable_file(&session.user_id, &file_id, &folder).await?;
    }

    Ok(FolderBody(
      assert_valid_folder(&session.user_id, &folder).await?,
      serde_json::from_value(body)?,
    ))
  }
}

/// Asserts:
/// * File with `folder_id` exists in files collection
/// * File with `folder_id` is a folder
/// * File belongs to user with `user_id`
///
/// Returns the `folder_id`, if root alias is used it returns the root folder_id
async fn assert_valid_folder(
  user_id: &str,
  folder_id: &Option<String>,
) -> APIResult<Option<String>> {
  let mut result = folder_id.clone();
  if let Some(folder_id) = folder_id.as_deref() {
    let folder_id = db::UserFile::map_folder_id(user_id, folder_id);
    let folder = db::DATABASE
      .find_by_id::<db::UserFile>(folder_id)
      .await?
      .ok_or_else(|| {
        APIError::BadRequest(f!("Folder with id {folder_id:?} does not exist"))
      })?;

    if !matches!(folder.metadata, db::FileMetadata::Folder) {
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

async fn assert_writable_file(
  user_id: &str,
  file_id: &str,
  folder_id: &Option<String>,
) -> APIResult {
  if file_id == user_id {
    return Err(APIError::UnauthorizedMessage(
      "Root folders are read-only".to_string(),
    ));
  }

  if let Some(folder_id) = folder_id {
    if file_id == folder_id {
      return Err(APIError::BadRequest(
        "A folder cannot be inside itself".to_string(),
      ));
    }
  }

  Ok(())
}
