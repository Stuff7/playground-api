use crate::{
  api::{self, APIError, APIResult},
  auth::session::{FileId, FileIdVecQuery, Session},
  console::Colorize,
  db::files::{
    aggregations::{FolderChildrenAndAncestors, FolderWithChildren},
    system::FileSystem,
    File, PartialFile, Video,
  },
  http::stream_video,
  log,
  websockets::{
    channel::{EventMessage, EventSender},
    WebSocketState,
  },
  AppResult, AppState,
};
use axum::{
  extract::{Path, Query, State},
  http::HeaderMap,
  response::IntoResponse,
  routing, Json, Router,
};
use format as f;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct FilesRouterState {
  request_client: reqwest::Client,
}

impl FilesRouterState {
  pub fn new() -> Self {
    Self {
      request_client: reqwest::Client::new(),
    }
  }
}

pub fn api() -> AppResult<Router<AppState>> {
  Ok(
    Router::new()
      .route("/", routing::get(get_files))
      .route("/", routing::delete(delete_files))
      .route("/:file_id", routing::patch(update_file))
      .route("/folder", routing::post(create_folder))
      .route("/folder/:folder_id", routing::get(get_folder_family))
      .route("/folder/move", routing::put(move_files))
      .route("/video/metadata", routing::get(get_video_metadata))
      .route("/video/:video_id", routing::get(stream))
      .route("/video/:video_id", routing::post(create_video)),
  )
}

pub async fn stream(
  Path(video_id): Path<String>,
  headers: HeaderMap,
) -> APIResult<impl IntoResponse> {
  stream_video(
    &f!(
      "https://drive.google.com/uc?export=download&confirm=yTib&id={video_id}"
    ),
    headers,
  )
  .await
}

pub async fn get_files(
  State(file_system): State<FileSystem>,
  query: PartialFile,
) -> APIResult<Json<Vec<File>>> {
  Ok(Json(file_system.find_many(&query).await?))
}

pub async fn get_folder_family(
  session: Session,
  State(file_system): State<FileSystem>,
  Path(folder_id): Path<String>,
) -> APIResult<Json<FolderChildrenAndAncestors>> {
  Ok(Json(
    file_system
      .find_children_and_ancestors(&session.user_id, &folder_id)
      .await?
      .ok_or_else(|| {
        APIError::NotFound(f!("Folder with id {folder_id:?} not found"))
      })?,
  ))
}

#[derive(Debug, Deserialize)]
pub struct CreateVideoBody {
  folder: Option<String>,
  name: Option<String>,
  thumbnail: Option<String>,
}

pub async fn create_video(
  session: Session,
  Path(video_id): Path<String>,
  State(FilesRouterState { request_client }): State<FilesRouterState>,
  State(WebSocketState { event_sender }): State<WebSocketState>,
  State(file_system): State<FileSystem>,
  Json(body): Json<CreateVideoBody>,
) -> APIResult<Json<File>> {
  let mut metadata = fetch_video_metadata(&request_client, &video_id).await?;

  if let Some(thumbnail) = body.thumbnail {
    metadata.thumbnail = thumbnail;
  }

  let (new_file, changes) = file_system
    .create_one(&File::from_video(
      metadata,
      session.user_id,
      body.folder,
      body.name,
    )?)
    .await?;
  send_folder_changes(&event_sender, changes)?;
  Ok(Json(new_file))
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderBody {
  folder: Option<String>,
  name: String,
}

pub async fn create_folder(
  session: Session,
  State(WebSocketState { event_sender }): State<WebSocketState>,
  State(file_system): State<FileSystem>,
  Json(body): Json<CreateFolderBody>,
) -> APIResult<Json<File>> {
  let (new_file, changes) = file_system
    .create_one(&File::new_folder(session.user_id, body.name, body.folder)?)
    .await?;
  send_folder_changes(&event_sender, changes)?;
  Ok(Json(new_file))
}

#[derive(Debug, Deserialize)]
pub struct MoveFilesBody {
  files: HashSet<String>,
  folder: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFilesResponse {
  moved_count: u64,
}

pub async fn move_files(
  session: Session,
  State(WebSocketState { event_sender }): State<WebSocketState>,
  State(file_system): State<FileSystem>,
  Json(body): Json<MoveFilesBody>,
) -> APIResult<Json<MoveFilesResponse>> {
  let (result, changes) = file_system
    .move_many(&session.user_id, &body.files, &body.folder)
    .await?;

  if let Some(changes) = changes {
    send_folder_changes(&event_sender, changes)?;
  }

  Ok(Json(MoveFilesResponse {
    moved_count: result.modified_count,
  }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateFileBody {
  name: Option<String>,
  folder: Option<String>,
}

pub async fn update_file(
  session: Session,
  State(WebSocketState { event_sender }): State<WebSocketState>,
  State(file_system): State<FileSystem>,
  FileId(file_id): FileId,
  Json(body): Json<UpdateFileBody>,
) -> APIResult<Json<File>> {
  let (file, changes) = file_system
    .update_one(&session.user_id, &file_id, body.folder, body.name)
    .await?;

  log!("CHANGES => {changes:#?}");
  send_folder_changes(&event_sender, changes)?;

  Ok(Json(file))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFilesResponse {
  deleted: u64,
}

pub async fn delete_files(
  session: Session,
  State(WebSocketState { event_sender }): State<WebSocketState>,
  State(file_system): State<FileSystem>,
  FileIdVecQuery(query): FileIdVecQuery,
) -> APIResult<Json<DeleteFilesResponse>> {
  let (deleted, changes) =
    file_system.delete_many(&session.user_id, &query).await?;

  send_folder_changes(&event_sender, changes)?;

  Ok(Json(DeleteFilesResponse { deleted }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetFileMetadataQuery {
  video_id: String,
}

pub async fn get_video_metadata(
  State(FilesRouterState { request_client }): State<FilesRouterState>,
  Query(GetFileMetadataQuery { video_id }): Query<GetFileMetadataQuery>,
) -> APIResult<Json<Video>> {
  Ok(Json(
    fetch_video_metadata(&request_client, &video_id).await?,
  ))
}

async fn fetch_video_metadata(
  request_client: &reqwest::Client,
  file_url: &str,
) -> APIResult<Video> {
  let video_id = if file_url.contains('/') {
    extract_drive_file_id(file_url).ok_or(APIError::BadRequest(f!(
      "Could not get file id from url {file_url:?}."
    )))?
  } else {
    file_url.to_string()
  };
  let file_data = api::google::get_file(&video_id, request_client).await?;
  let video_metadata = file_data.video_metadata.ok_or_else(|| {
    APIError::BadRequest(f!(
      "Found file for file id {video_id:?} with name {:?} but is not a video",
      file_data.name
    ))
  })?;
  Ok(Video {
    play_id: video_id.clone(),
    name: file_data.name,
    width: video_metadata.width,
    height: video_metadata.height,
    duration_millis: video_metadata.duration_millis,
    mime_type: file_data.mime_type,
    size_bytes: file_data.size_bytes.unwrap_or_default(),
    thumbnail: api::google::thumbnail_url(&video_id),
  })
}

fn extract_drive_file_id(share_link: &str) -> Option<String> {
  share_link.find("file/d/").and_then(|start| {
    let slice = &share_link[(start + 7)..];
    slice.find('/').map(|end| slice[..end].to_string())
  })
}

fn send_folder_changes(
  event_sender: &EventSender,
  changes: Vec<FolderWithChildren>,
) -> APIResult {
  if event_sender.receiver_count() == 0 {
    log!(info@"There's {} folder changes but no one's listening. Message will not be sent", changes.len());
  } else {
    log!(info@"Sending message to {} listeners", event_sender.receiver_count());
    for change in changes.into_iter() {
      event_sender.send(EventMessage::FolderChange(change))?;
    }
  }
  Ok(())
}
