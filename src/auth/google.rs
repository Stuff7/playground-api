use crate::api::APIError;
use crate::api::APIResult;
use crate::console::Colorize;
use crate::db;
use crate::db::Cache;
use crate::env_var;
use crate::http::get_range;
use crate::http::json_response;
use crate::http::JsonResult;
use crate::log;
use crate::AppResult;

use super::oauth::Token;
use super::session::Session;
use super::AuthQuery;
use super::AuthenticateQuery;
use super::AuthorizedQuery;
use super::UserIDQuery;

use format as f;

use std::collections::HashMap;

use axum::extract::Path;
use axum::http::HeaderMap;
use axum::{
  extract::{FromRef, Query, State},
  http::StatusCode,
  response::{IntoResponse, Redirect},
  routing::get,
  Json, Router,
};
use oauth2::{
  basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FileCache {
  size: String,
  mime_type: String,
}

const FILE_FIELDS: &str = "id,name,kind,size,videoMediaMetadata,mimeType";
// To avoid having to request video metadata on every video streaming request.
static FILE_CACHE: Cache<FileCache> = Lazy::new(|| Mutex::new(HashMap::new()));
static OAUTH_CACHE: Cache<String> = Lazy::new(|| Mutex::new(HashMap::new()));

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
      .route("/drive/files", get(drive_files))
      .route("/drive/files/:file_id", get(drive_file))
      .route("/drive/video/:video_id", get(drive_video))
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
async fn authenticate(
  State(client): State<BasicClient>,
  Query(query): Query<AuthenticateQuery>,
) -> Redirect {
  let (auth_url, csrf_token) = client
    .authorize_url(CsrfToken::new_random)
    .add_scope(scope("auth/userinfo.email"))
    .add_scope(scope("auth/userinfo.profile"))
    .add_scope(Scope::new("openid".to_string()))
    // Sensitive scopes
    .add_scope(scope("auth/drive.readonly"))
    .url();

  if let Some(login) = query.current_login {
    OAUTH_CACHE
      .lock()
      .await
      .insert(csrf_token.secret().to_string(), login);
  }
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
  let provider = db::Provider::new(
    f!(
      "google@{}",
      profile
        .email
        .split_once('@')
        .ok_or_else(|| APIError::Internal(f!(
          "Invalid email from google provider {:?}",
          profile.email
        )))?
        .0
    ),
    profile.picture,
    token.access_token,
    token.refresh_token,
    token.expires_seconds,
  );

  // Check if provider authentication was done by a logged in user
  let logged_in_user = match OAUTH_CACHE.lock().await.remove(&query.state) {
    Some(token) => match Session::from_token(&token).await {
      Ok(session) => {
        let user = session.get_user().await.ok();
        Session::invalidate(&token).await;
        user
      }
      Err(_) => None,
    },
    None => None,
  };

  let token = match logged_in_user {
    Some(user) => db::add_provider_to_user(user, provider).await?,
    None => {
      db::save_user(
        &db::User::new(&provider._id, &profile.name, &provider.picture),
        provider,
      )
      .await?
    }
  };

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoMetadata {
  width: Option<usize>,
  height: Option<usize>,
  #[serde(alias = "durationMillis")]
  duration_millis: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleDriveFile {
  kind: String,
  id: String,
  #[serde(alias = "mimeType")]
  mime_type: String,
  name: String,
  size: Option<String>,
  #[serde(alias = "videoMediaMetadata")]
  video_metadata: Option<VideoMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleDriveFilesResponse {
  files: Vec<GoogleDriveFile>,
}

/// Get all drive files from specific google account.
async fn request_drive_files(
  client: &BasicClient,
  user: &db::User,
  user_id: &str,
) -> APIResult<GoogleDriveFilesResponse> {
  let response = drive_request(
    client,
    user,
    user_id,
    Request::Endpoint(&f!(
      "files?fields=files({FILE_FIELDS})&trashed=false&orderBy=quotaBytesUsed desc"
    )),
  )
  .await?;
  return match json_response::<GoogleDriveFilesResponse>(response).await? {
    JsonResult::Typed(file) => Ok(file),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file)),
  };
}

type DriveFilesResponse = HashMap<String, Vec<GoogleDriveFile>>;

/// List all drive files for single google account or all user linked accounts if user is not passed in.
async fn drive_files(
  Query(query): Query<UserIDQuery>,
  State(client): State<BasicClient>,
  session: Session,
) -> APIResult<Json<DriveFilesResponse>> {
  let mut files_response: DriveFilesResponse = HashMap::new();
  let user = session.get_user().await?;
  match query.user {
    Some(user_id) => {
      let files = request_drive_files(&client, &user, &user_id).await?.files;
      files_response.insert(user_id, files);
    }
    None => {
      for user_id in user.linked_accounts.clone() {
        let files = request_drive_files(&client, &user, &user_id).await?.files;
        files_response.insert(user_id, files);
      }
    }
  }
  Ok(Json(files_response))
}

/// Get drive file from specific google account or current user if user query param is not passed in.
async fn drive_file(
  Query(query): Query<UserIDQuery>,
  Path(file_id): Path<String>,
  State(client): State<BasicClient>,
  session: Session,
) -> APIResult<Json<GoogleDriveFile>> {
  let user = session.get_user().await?;
  let response = drive_request(
    &client,
    &user,
    &query.user.unwrap_or_else(|| user._id.clone()),
    Request::Endpoint(&f!("files/{file_id}?fields={FILE_FIELDS}&trashed=false")),
  )
  .await?;
  match json_response::<GoogleDriveFile>(response).await? {
    JsonResult::Typed(file) => Ok(Json(file)),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file)),
  }
}

/// Download auth protected video and send response for video players.
async fn drive_video(
  Query(query): Query<AuthQuery>,
  Path(video_id): Path<String>,
  headers: HeaderMap,
  State(client): State<BasicClient>,
) -> APIResult<impl IntoResponse> {
  let (range_start, range_end) = get_range(headers);
  let byte_range = f!("{range_start}-{range_end}");
  let builder = reqwest::Client::new()
    .get(f!(
      "https://www.googleapis.com/drive/v3/files/{video_id}?alt=media"
    ))
    .header("Range", f!("bytes={byte_range}"));

  let user = Session::from_token(&query.token).await?.get_user().await?;
  let user_id = &query.user.unwrap_or_else(|| user._id.clone());
  let response = drive_request(&client, &user, user_id, Request::Builder(builder)).await?;

  let (content_length, content_type) = {
    let mut cache = FILE_CACHE.lock().await;
    match cache.get(&video_id).cloned() {
      Some(metadata) => (metadata.size, metadata.mime_type),
      None => {
        let response = drive_request(
          &client,
          &user,
          user_id,
          Request::Endpoint(&f!("files/{video_id}?fields={FILE_FIELDS}&trashed=false")),
        )
        .await?;
        match json_response::<GoogleDriveFile>(response).await? {
          JsonResult::Typed(file) => {
            log!("CACHING FILE METADATA {video_id}");
            let GoogleDriveFile {
              mime_type, size, ..
            } = file;
            cache.insert(
              video_id,
              FileCache {
                mime_type: mime_type.clone(),
                size: size.clone().unwrap_or_default(),
              },
            );
            (size.unwrap_or_default(), mime_type)
          }
          JsonResult::Untyped(file) => return Err(APIError::JsonParsing(file)),
        }
      }
    }
  };
  let body = response.bytes().await?;

  let mut headers = HeaderMap::new();
  headers.insert("Accept-Ranges", "bytes".parse()?);
  headers.insert(
    "Content-Range",
    f!("bytes {byte_range}/{content_length}").parse()?,
  );
  headers.insert("Content-Type", content_type.parse()?);

  Ok((StatusCode::PARTIAL_CONTENT, headers, body))
}

enum Request<T: std::fmt::Display> {
  Endpoint(T),
  Builder(reqwest::RequestBuilder),
}

/// Request auth protected drive API stuff.
async fn drive_request(
  client: &BasicClient,
  user: &db::User,
  user_id: &str,
  request: Request<&str>,
) -> APIResult<reqwest::Response> {
  if !user.linked_accounts.contains(user_id) {
    return Err(APIError::UnauthorizedMessage(f!(
      "user with id {:?} is not authorized to see {:?}",
      user._id,
      user_id
    )));
  }

  let mut token = {
    let provider = db::find_by_id::<db::Provider>(user_id)
      .await
      .ok_or_else(|| {
        APIError::UnauthorizedMessage(f!(
          "Could not find provider {:?}. Try logging in with your google account",
          user_id
        ))
      })?;
    provider.token
  };

  let access_token = token.access_token.clone();

  let request = match request {
    Request::Endpoint(endpoint) => {
      reqwest::Client::new().get(f!("https://www.googleapis.com/drive/v3/{endpoint}"))
    }
    Request::Builder(builder) => builder,
  };
  let response = token.request(client, request).await?;

  // if access token changed after the request, it means it was refreshed
  if access_token != token.access_token {
    log!(info@"ACCESS TOKEN REFRESHED UPDATING DATABASE PROVIDER {user_id:?}");
    db::update_provider_token(user_id, token).await?;
  }

  Ok(response)
}
