use crate::api::APIError;
use crate::api::APIResult;
use crate::db;
use crate::env_var;
use crate::http::json_response;
use crate::http::JsonResult;
use crate::AppResult;

use super::oauth::Token;
use super::session::Session;
use super::AuthorizedQuery;

use format as f;

use axum::{
  extract::{FromRef, Query, State},
  response::Redirect,
  routing::get,
  Router,
};
use oauth2::{
  basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
  oauth_client: BasicClient,
  login_redirect: String,
}

impl FromRef<AppState> for BasicClient {
  fn from_ref(state: &AppState) -> Self {
    state.oauth_client.clone()
  }
}

impl FromRef<AppState> for String {
  fn from_ref(state: &AppState) -> Self {
    state.login_redirect.clone()
  }
}

/// Setup API endpoints for google services.
pub fn api() -> AppResult<Router> {
  let oauth_client = oauth_client()?;
  let login_redirect = env_var("LOGIN_REDIRECT")?;

  let app_state = AppState {
    oauth_client,
    login_redirect,
  };

  Ok(
    Router::new()
      .route("/login", get(authenticate))
      .route("/authorized", get(login_authorized))
      .with_state(app_state),
  )
}

/// Create Google OAuth client to interact with Google APIs.
fn oauth_client() -> AppResult<BasicClient> {
  let client_id = env_var("GOOGLE_CLIENT_ID")?;
  let client_secret = env_var("GOOGLE_CLIENT_SECRET")?;
  let redirect_url = env_var("GOOGLE_REDIRECT_URL")?;

  let auth_url = "https://accounts.google.com/o/oauth2/v2/auth?access_type=offline".to_string();
  let token_url = "https://oauth2.googleapis.com/token".to_string();

  Ok(
    BasicClient::new(
      ClientId::new(client_id),
      Some(ClientSecret::new(client_secret)),
      AuthUrl::new(auth_url)?,
      Some(TokenUrl::new(token_url)?),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_url)?),
  )
}

/// Redirect to Google's OAuth consent screen.
async fn authenticate(State(client): State<BasicClient>) -> Redirect {
  let (auth_url, _) = client
    .authorize_url(CsrfToken::new_random)
    .add_scope(scope("auth/userinfo.email"))
    .add_scope(scope("auth/userinfo.profile"))
    .add_scope(Scope::new("openid".to_string()))
    .url();

  // Redirect to Google's oauth service
  Redirect::to(auth_url.as_ref())
}

/// Create google API scope.
fn scope(scope_name: &str) -> Scope {
  Scope::new(f!("https://www.googleapis.com/{scope_name}"))
}

#[derive(Debug, Serialize, Deserialize)]
struct APITokenResponse {
  token: String,
}

/// Add/update provider and user.
async fn login_authorized(
  Query(query): Query<AuthorizedQuery>,
  State(client): State<BasicClient>,
  State(login_redirect): State<String>,
) -> APIResult<Redirect> {
  let token = Token::exchange(&client, query.code).await?;

  let profile = google_user_info(&token.access_token).await?;
  let id = f!(
    "google@{}",
    profile
      .email
      .split_once('@')
      .ok_or_else(|| APIError::Internal(f!(
        "Invalid email from google provider {:?}",
        profile.email
      )))?
      .0
  );

  let token = db::save_user(&db::User::new(&id, &profile.name, &profile.picture)).await?;

  Session::save(&token).await;

  Ok(Redirect::to(&f!("{login_redirect}?access_token={token}")))
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleUserInfo {
  email: String,
  name: String,
  picture: String,
}

/// Request auth protected basic user info from google.
async fn google_user_info(access_token: &str) -> APIResult<GoogleUserInfo> {
  let client = reqwest::Client::new();
  let url = f!("https://www.googleapis.com/oauth2/v3/userinfo?access_token={access_token}");
  let response = client.get(url).bearer_auth(access_token).send().await?;

  match json_response::<GoogleUserInfo>(response).await? {
    JsonResult::Typed(profile) => Ok(profile),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file)),
  }
}
