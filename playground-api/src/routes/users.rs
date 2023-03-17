use crate::auth::session::Session;
use crate::db::users::User;
use crate::db::Database;
use crate::{api::APIResult, AppState};

use axum::extract::State;
use axum::{routing::get, Json, Router};

pub fn api() -> Router<AppState> {
  Router::new().route("/me", get(current_user))
}

async fn current_user(
  session: Session,
  State(database): State<Database>,
) -> APIResult<Json<User>> {
  Ok(Json(session.get_user(&database).await?))
}
