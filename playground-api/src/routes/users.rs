use crate::api::APIResult;
use crate::auth::session::Session;
use crate::db;

use axum::{routing::get, Json, Router};

pub fn api() -> Router {
  Router::new().route("/me", get(current_user))
}

async fn current_user(session: Session) -> APIResult<Json<db::User>> {
  Ok(Json(session.get_user().await?))
}
