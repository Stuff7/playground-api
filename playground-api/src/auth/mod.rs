pub mod google;
pub mod jwt;
pub mod oauth;
pub mod session;

use crate::{AppResult, AppState};
use axum::Router;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AuthorizedQuery {
  code: String,
}

pub fn api() -> AppResult<Router<AppState>> {
  Ok(Router::new().nest("/google", google::api()?))
}
