use std::collections::HashSet;

use crate::{
  api::{self, APIError, APIResult},
  auth::session::{FileId, FileIdVecQuery, FolderBody, Session},
  console::Colorize,
  db::{
    self,
    files::{
      queries::{FolderChange, FolderFamily},
      File, PartialFile, Video,
    },
  },
  http::stream_video,
  log,
  websockets::channel::{EventMessage, EventSender},
  AppResult,
};

use axum::{
  extract::{FromRef, Path, Query, State},
  http::HeaderMap,
  response::IntoResponse,
};
use axum::{routing, Json, Router};
use serde::{Deserialize, Serialize};

use format as f;

#[derive(Clone)]
pub struct FilesRouterState {
  event_sender: EventSender,
  request_client: reqwest::Client,
}

impl FromRef<FilesRouterState> for reqwest::Client {
  fn from_ref(state: &FilesRouterState) -> Self {
    state.request_client.clone()
  }
}

impl FromRef<FilesRouterState> for EventSender {
  fn from_ref(state: &FilesRouterState) -> Self {
    state.event_sender.clone()
  }
}

pub fn api(event_sender: EventSender) -> AppResult<Router> {
  let state = FilesRouterState {
    event_sender,
    request_client: reqwest::Client::new(),
  };
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
      .route("/video/:video_id", routing::post(create_video))
      .with_state(state),
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

pub async fn get_files(query: PartialFile) -> APIResult<Json<Vec<File>>> {
  let files = db::DATABASE
    .find_many::<File>(File::query(&query)?)
    .await
    .unwrap_or_default();
  Ok(Json(files))
}

pub async fn get_folder_family(
  session: Session,
  Path(folder_id): Path<String>,
) -> APIResult<Json<FolderFamily>> {
  Ok(Json(
    File::get_folder_family(&session.user_id, &folder_id)
      .await?
      .ok_or_else(|| {
        APIError::NotFound(f!("Folder with id {folder_id:?} not found"))
      })?,
  ))
}

#[derive(Debug, Deserialize)]
pub struct CreateVideoBody {
  name: Option<String>,
  thumbnail: Option<String>,
}

pub async fn create_video(
  session: Session,
  Path(video_id): Path<String>,
  State(request_client): State<reqwest::Client>,
  State(event_sender): State<EventSender>,
  FolderBody(folder, body): FolderBody<CreateVideoBody>,
) -> APIResult<Json<File>> {
  let mut metadata = fetch_video_metadata(&request_client, &video_id).await?;

  if let Some(thumbnail) = body.thumbnail {
    metadata.thumbnail = thumbnail;
  }

  let (new_file, changes) = File::create_one(&File::from_video(
    metadata,
    session.user_id,
    folder,
    body.name,
  )?)
  .await?;
  send_folder_changes(&event_sender, changes)?;
  Ok(Json(new_file))
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderBody {
  name: String,
}

pub async fn create_folder(
  session: Session,
  State(event_sender): State<EventSender>,
  FolderBody(folder, body): FolderBody<CreateFolderBody>,
) -> APIResult<Json<File>> {
  let (new_file, changes) =
    File::create_one(&File::new_folder(session.user_id, body.name, folder)?)
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
  State(event_sender): State<EventSender>,
  Json(body): Json<MoveFilesBody>,
) -> APIResult<Json<MoveFilesResponse>> {
  let (result, changes) =
    File::move_many(&session.user_id, &body.files, &body.folder).await?;

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
  State(event_sender): State<EventSender>,
  FileId(file_id): FileId,
  Json(body): Json<UpdateFileBody>,
) -> APIResult<Json<File>> {
  let (file, changes) =
    File::update_one(&session.user_id, &file_id, body.folder, body.name)
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
  State(event_sender): State<EventSender>,
  FileIdVecQuery(mut query): FileIdVecQuery,
) -> APIResult<Json<DeleteFilesResponse>> {
  query.remove(&session.user_id);
  let Some(result) =
    File::query_nested_files(&session.user_id, &query).await? else {
      return Ok(Json(DeleteFilesResponse { deleted: 0 }))
    };

  let deleted = db::DATABASE
    .delete_many::<File>(File::query_many_by_id(&session.user_id, &result.ids)?)
    .await?;
  let changes = File::lookup_folder_files(&File::query_many_by_id(
    &session.user_id,
    &result.folder_ids,
  )?)
  .await?;

  log!(info@"CHANGES => {changes:#?}");
  send_folder_changes(&event_sender, changes)?;

  Ok(Json(DeleteFilesResponse { deleted }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetFileMetadataQuery {
  video_id: String,
}

pub async fn get_video_metadata(
  State(request_client): State<reqwest::Client>,
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
  changes: Vec<FolderChange>,
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
