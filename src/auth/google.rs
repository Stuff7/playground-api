use crate::api::APIError;
use crate::api::APIResponseResult;
use crate::console::Colorize;
use crate::db;
use crate::db::get_provider_by_id;
use crate::db::update_provider_token;
use crate::env_var;
use crate::http::get_range;
use crate::http::json_response;
use crate::http::JsonResult;

use super::jwt::decode_token;
use super::oauth::Token;
use super::AuthQuery;
use super::AuthRequest;
use super::UserIDQuery;

use axum::headers::authorization::Bearer;
use axum::headers::Authorization;
use axum::TypedHeader;
use format as f;

use std::collections::HashMap;

use anyhow::Context;
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
}

impl FromRef<AppState> for BasicClient {
  fn from_ref(state: &AppState) -> Self {
    state.oauth_client.clone()
  }
}

type Cache<T> = Lazy<Mutex<HashMap<String, T>>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FileCache {
  size: String,
  mime_type: String,
}

const FILE_FIELDS: &str = "id,name,kind,size,videoMediaMetadata,mimeType";
static FILE_CACHE: Cache<FileCache> = Lazy::new(|| Mutex::new(HashMap::new()));
static TOKEN_CACHE: Cache<Token> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn api() -> anyhow::Result<Router> {
  let oauth_client = oauth_client()?;
  let app_state = AppState { oauth_client };

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

fn oauth_client() -> anyhow::Result<BasicClient> {
  let client_id = env_var("GOOGLE_CLIENT_ID")?;
  let client_secret = env_var("GOOGLE_CLIENT_SECRET")?;
  let redirect_url = env_var("GOOGLE_REDIRECT_URL")?;

  let auth_url = "https://accounts.google.com/o/oauth2/v2/auth?access_type=offline".to_string();
  let token_url = "https://oauth2.googleapis.com/token".to_string();

  Ok(
    BasicClient::new(
      ClientId::new(client_id),
      Some(ClientSecret::new(client_secret)),
      AuthUrl::new(auth_url).context("Failed to parse auth url".err())?,
      Some(TokenUrl::new(token_url).context("Failed to parse token url".err())?),
    )
    .set_redirect_uri(
      RedirectUrl::new(redirect_url).context("Failed to parse redirect url".err())?,
    ),
  )
}

async fn authenticate(State(client): State<BasicClient>) -> impl IntoResponse {
  let (auth_url, _csrf_token) = client
    .authorize_url(CsrfToken::new_random)
    .add_scope(scope("auth/userinfo.email"))
    .add_scope(scope("auth/userinfo.profile"))
    .add_scope(Scope::new("openid".to_string()))
    .add_scope(scope("auth/drive.appdata"))
    .add_scope(scope("auth/drive.file"))
    .add_scope(scope("auth/drive.install"))
    // Sensitive scopes
    .add_scope(scope("auth/drive.metadata.readonly"))
    .add_scope(scope("auth/drive.photos.readonly"))
    .add_scope(scope("auth/drive.readonly"))
    .url();

  // Redirect to Google's oauth service
  Redirect::to(auth_url.as_ref())
}

fn scope(scope_name: &str) -> Scope {
  Scope::new(f!("https://www.googleapis.com/{scope_name}"))
}

#[derive(Debug, Serialize, Deserialize)]
struct APITokenResponse {
  token: String,
}

async fn login_authorized(
  Query(query): Query<AuthRequest>,
  State(client): State<BasicClient>,
) -> APIResponseResult<impl IntoResponse> {
  let token = Token::exchange(&client, query.code).await?;

  let profile = google_user_info(&token.access_token).await?;
  let provider = db::Provider::new(
    f!("google#{}", profile.email),
    token.access_token,
    token.refresh_token,
    token.expires_seconds,
  );

  let token = db::save_user(provider).await?;

  Ok(Json(APITokenResponse { token }))
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleUserInfo {
  #[serde(alias = "emailAddress")]
  email: String,
}

async fn google_user_info(access_token: &str) -> APIResponseResult<GoogleUserInfo> {
  let client = reqwest::Client::new();
  let url = f!("https://www.googleapis.com/oauth2/v3/userinfo?access_token={access_token}");
  let response = client.get(url).bearer_auth(access_token).send().await?;

  match json_response::<GoogleUserInfo>(response).await? {
    JsonResult::Typed(profile) => Ok(profile),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file).into()),
  }
}

#[derive(Debug, Serialize, Deserialize)]
struct VideoMetadata {
  width: Option<usize>,
  height: Option<usize>,
  #[serde(alias = "durationMillis")]
  duration_millis: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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

async fn drive_files(
  TypedHeader(bearer): TypedHeader<Authorization<Bearer>>,
  Query(query): Query<UserIDQuery>,
  State(client): State<BasicClient>,
) -> APIResponseResult<impl IntoResponse> {
  let response = drive_request(
    &client,
    bearer.token(),
    &query.user,
    Request::Endpoint(&f!("files?fields=files({FILE_FIELDS})&trashed=false")),
  )
  .await?;

  match json_response::<GoogleDriveFilesResponse>(response).await? {
    JsonResult::Typed(file) => Ok(Json(file)),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file).into()),
  }
}

async fn drive_file(
  TypedHeader(bearer): TypedHeader<Authorization<Bearer>>,
  Query(query): Query<UserIDQuery>,
  Path(file_id): Path<String>,
  State(client): State<BasicClient>,
) -> APIResponseResult<impl IntoResponse> {
  let response = drive_request(
    &client,
    bearer.token(),
    &query.user,
    Request::Endpoint(&f!("files/{file_id}?fields={FILE_FIELDS}&trashed=false")),
  )
  .await?;

  match json_response::<GoogleDriveFile>(response).await? {
    JsonResult::Typed(file) => Ok(Json(file)),
    JsonResult::Untyped(file) => Err(APIError::JsonParsing(file).into()),
  }
}

async fn drive_video(
  Query(query): Query<AuthQuery>,
  Path(video_id): Path<String>,
  headers: HeaderMap,
  State(client): State<BasicClient>,
) -> APIResponseResult<impl IntoResponse> {
  let (range_start, range_end) = get_range(headers);
  let byte_range = f!("{range_start}-{range_end}");
  let builder = reqwest::Client::new()
    .get(f!(
      "https://www.googleapis.com/drive/v3/files/{video_id}?alt=media"
    ))
    .header("Range", f!("bytes={byte_range}"));

  let response = drive_request(
    &client,
    &query.token,
    &query.user,
    Request::Builder(builder),
  )
  .await?;

  let (content_length, content_type) = {
    let mut cache = FILE_CACHE.lock().await;
    match cache.get(&video_id).cloned() {
      Some(metadata) => (metadata.size, metadata.mime_type),
      None => {
        let response = drive_request(
          &client,
          &query.token,
          &query.user,
          Request::Endpoint(&f!("files/{video_id}?fields={FILE_FIELDS}&trashed=false")),
        )
        .await?;
        match json_response::<GoogleDriveFile>(response).await? {
          JsonResult::Typed(file) => {
            println!("{}", f!("CACHING FILE METADATA {video_id}").info());
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
          JsonResult::Untyped(file) => return Err(APIError::JsonParsing(file).into()),
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

async fn drive_request(
  client: &BasicClient,
  token: &str,
  user_id: &str,
  request: Request<&str>,
) -> APIResponseResult<reqwest::Response> {
  let user = decode_token::<db::User>(token)?.claims;

  if !user.linked_accounts.contains(user_id) {
    return Err(
      APIError::Unauthorized(f!(
        "user with id {:?} is not authorized to see {:?}",
        user._id,
        user_id
      ))
      .into(),
    );
  }

  let mut token = {
    let mut cache = TOKEN_CACHE.lock().await;
    match cache.get(user_id).cloned() {
      Some(token) => token,
      None => {
        let provider = get_provider_by_id(user_id).ok_or_else(|| {
          APIError::Unauthorized(f!(
            "Could not find provider {:?}. Try logging in with your google account",
            user_id
          ))
        })?;
        println!("{}", f!("CACHING TOKEN {}", user_id).info());
        cache.insert(user_id.to_owned(), provider.token.clone());
        provider.token
      }
    }
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
    println!(
      "{}",
      f!(
        "ACCESS TOKEN REFRESHED UPDATING DATABASE PROVIDER {:?}",
        user_id
      )
      .info()
    );
    TOKEN_CACHE
      .lock()
      .await
      .insert(user_id.to_owned(), token.clone());
    update_provider_token(user_id, token).await?;
  }

  Ok(response)
}
