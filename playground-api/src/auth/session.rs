use crate::{
  api::{APIError, APIResult},
  db, GracefulExit,
};

use axum::{
  async_trait,
  extract::{FromRequestParts, Path, Query, TypedHeader},
  headers::{authorization::Bearer, Authorization},
  http::request::Parts,
  RequestPartsExt,
};
use serde::{Deserialize, Serialize};

use super::jwt;

use format as f;

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionQuery {
  pub folder: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Session {
  pub user_id: String,
  pub query: SessionQuery,
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

  pub async fn from_token(token: &str, mut query: SessionQuery) -> APIResult<Self> {
    let mut cache = db::SESSIONS_CACHE.lock().await;
    let user_id = cache
      .contains(token)
      .then(|| jwt::verify_token(token).map(|token| token.claims.sub))
      .ok_or_else(|| APIError::UnauthorizedMessage("Invalid session".to_string()))?
      .map_err(|err| {
        cache.remove(token);
        APIError::from(err)
      })?;
    query.folder = assert_valid_folder(&user_id, &query.folder).await?;
    Ok(Self { user_id, query })
  }
}

#[async_trait]
impl<S> FromRequestParts<S> for Session
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
    let bearer: Option<TypedHeader<Authorization<Bearer>>> = parts
      .extract()
      .await
      .unwrap_or_exit("Could not extract Authorization header");

    let token = bearer
      .map(|bearer| bearer.token().to_string())
      .ok_or_else(|| {
        APIError::UnauthorizedMessage("Missing/Invalid Authorization header".to_string())
      })?;

    let Query(query) = parts.extract::<Query<SessionQuery>>().await?;

    Ok(Self::from_token(&token, query).await?)
  }
}

pub struct SessionWithFileId(pub Session, pub String);

#[async_trait]
impl<S> FromRequestParts<S> for SessionWithFileId
where
  S: Send + Sync,
{
  type Rejection = APIError;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let session = Session::from_request_parts(parts, state).await?;
    let Path(file_id) = parts.extract::<Path<String>>().await?;

    assert_writable_file(&session.user_id, &file_id, &session.query.folder).await?;
    Ok(SessionWithFileId(session, file_id))
  }
}

async fn assert_valid_folder(
  user_id: &str,
  folder_id: &Option<String>,
) -> APIResult<Option<String>> {
  let mut result = folder_id.clone();
  if let Some(folder_id) = folder_id.as_deref() {
    let folder_id = if folder_id == "root" {
      user_id
    } else {
      folder_id
    };
    let folder = db::DATABASE
      .find_by_id::<db::UserFile>(folder_id)
      .await?
      .ok_or_else(|| APIError::BadRequest(f!("Folder with id {folder_id:?} does not exist")))?;

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
