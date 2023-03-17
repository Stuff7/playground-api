pub mod google;
pub mod jwt;
pub mod oauth;
pub mod session;

use axum::Router;
use serde::Deserialize;

use crate::{AppResult, AppState};

#[derive(Debug, Deserialize)]
struct AuthorizedQuery {
  code: String,
}

pub fn api() -> AppResult<Router<AppState>> {
  Ok(Router::new().nest("/google", google::api()?))
}
