use crate::{
  api::{APIError, APIResult},
  db, GracefulExit,
};

use axum::{
  async_trait,
  extract::{FromRequestParts, TypedHeader},
  headers::{authorization::Bearer, Authorization},
  http::request::Parts,
  RequestPartsExt,
};
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashSet;
use tokio::sync::Mutex;

use super::jwt;

static SESSION_CACHE: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Serialize)]
pub struct Session {
  pub user_id: String,
}

impl Session {
  pub async fn get_user(&self) -> APIResult<db::User> {
    Ok(
      db::get_by_id::<db::User>(&self.user_id.as_ref())
        .await
        .ok_or_else(|| APIError::Unauthorized)?,
    )
  }

  pub async fn save(token: &str) {
    SESSION_CACHE.lock().await.insert(token.to_string());
  }

  pub async fn invalidate(token: &str) {
    SESSION_CACHE.lock().await.remove(token);
  }

  pub async fn from_token(token: &str) -> APIResult<Self> {
    let cache = SESSION_CACHE.lock().await;
    let user_id = cache
      .contains(token)
      .then(|| jwt::verify_token(&token).and_then(|token| Ok(token.claims.sub)))
      .ok_or_else(|| APIError::UnauthorizedMessage("Invalid session".to_string()))??;

    Ok(Self { user_id })
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

    let access_token = bearer
      .and_then(|bearer| Some(bearer.token().to_string()))
      .ok_or_else(|| {
        APIError::UnauthorizedMessage("Missing/Invalid Authorization header".to_string())
      })?;

    Ok(Self::from_token(&access_token).await?)
  }
}
