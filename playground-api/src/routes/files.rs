use std::collections::HashSet;

use crate::{
  api::{self, APIError, APIResult},
  auth::session::{FileId, FolderBody, FolderQuery, Session},
  db,
  http::stream_video,
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
pub struct VideoRouterState {
  request_client: reqwest::Client,
}

impl FromRef<VideoRouterState> for reqwest::Client {
  fn from_ref(state: &VideoRouterState) -> Self {
    state.request_client.clone()
  }
}

pub fn api() -> AppResult<Router> {
  let state = VideoRouterState {
    request_client: reqwest::Client::new(),
  };
  Ok(
    Router::new()
      .route("/", routing::get(get_files))
      .route("/:file_id", routing::patch(update_file))
      .route("/:file_id", routing::delete(delete_file))
      .route("/folder", routing::post(create_folder))
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
  State(request_client): State<reqwest::Client>,
) -> APIResult<impl IntoResponse> {
  stream_video(
    &f!("https://drive.google.com/uc?export=download&confirm=yTib&id={video_id}"),
    headers,
    &request_client,
  )
  .await
}

pub async fn get_files(
  session: Session,
  FolderQuery(folder): FolderQuery,
) -> APIResult<Json<Vec<db::UserFile>>> {
  let files = db::DATABASE
    .find_many::<db::UserFile>(db::UserFile::folder_query(session.user_id, folder)?)
    .await
    .unwrap_or_default();
  Ok(Json(files))
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
  FolderBody(folder, body): FolderBody<CreateVideoBody>,
) -> APIResult<Json<db::UserFile>> {
  let mut metadata = fetch_video_metadata(&request_client, &video_id).await?;
  if let Some(thumbnail) = body.thumbnail {
    metadata.thumbnail = thumbnail;
  }
  Ok(Json(
    save_file(&db::UserFile::from_video(
      metadata,
      session.user_id,
      folder,
      body.name,
    )?)
    .await?
    .clone(),
  ))
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderBody {
  name: String,
}

pub async fn create_folder(
  session: Session,
  FolderBody(folder, body): FolderBody<CreateFolderBody>,
) -> APIResult<Json<db::UserFile>> {
  Ok(Json(
    save_file(&db::UserFile::new_folder(
      session.user_id,
      body.name,
      folder,
    )?)
    .await?
    .clone(),
  ))
}

#[derive(Debug, Deserialize)]
pub struct MoveFilesBody {
  files: HashSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFilesResponse {
  moved_count: u64,
}

pub async fn move_files(
  session: Session,
  FolderBody(folder, mut body): FolderBody<MoveFilesBody>,
) -> APIResult<Json<MoveFilesResponse>> {
  let folder = folder.unwrap_or_else(|| session.user_id.clone());
  body.files.remove(&session.user_id);
  body.files.remove(&folder);
  let update = db::UserFile::query(&db::PartialUserFile {
    folder_id: Some(folder.clone()),
    ..Default::default()
  })?;
  let query = db::UserFile::files_query(session.user_id, &body.files)?;
  let result = db::DATABASE
    .update_many::<db::UserFile>(update, query)
    .await?;
  Ok(Json(MoveFilesResponse {
    moved_count: result.modified_count,
  }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateFileBody {
  name: Option<String>,
}

pub async fn update_file(
  session: Session,
  FileId(file_id): FileId,
  FolderBody(folder, body): FolderBody<UpdateFileBody>,
) -> APIResult<Json<db::UserFile>> {
  let update = db::UserFile::update_query(body.name, folder)?;
  let query = db::UserFile::user_query(file_id.clone(), session.user_id)?;
  Ok(Json(
    db::DATABASE
      .update::<db::UserFile>(update, query)
      .await?
      .ok_or_else(|| APIError::NotFound(f!("File with id {file_id:?} not found")))?,
  ))
}

pub async fn delete_file(
  session: Session,
  FileId(file_id): FileId,
) -> APIResult<Json<db::UserFile>> {
  Ok(Json(
    db::DATABASE
      .delete::<db::UserFile>(db::UserFile::user_query(file_id.clone(), session.user_id)?)
      .await?
      .ok_or_else(|| APIError::NotFound(f!("File with id {file_id:?} not found")))?,
  ))
}

#[derive(Debug, Deserialize)]
pub struct GetFileMetadataQuery {
  file_url: String,
}

pub async fn get_video_metadata(
  State(request_client): State<reqwest::Client>,
  Query(GetFileMetadataQuery { file_url }): Query<GetFileMetadataQuery>,
) -> APIResult<Json<db::Video>> {
  Ok(Json(
    fetch_video_metadata(&request_client, &file_url).await?,
  ))
}

async fn fetch_video_metadata(
  request_client: &reqwest::Client,
  file_url: &str,
) -> APIResult<db::Video> {
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
  Ok(db::Video {
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

async fn save_file(file: &db::UserFile) -> APIResult<&db::UserFile> {
  db::save_file(file).await?.ok_or_else(|| {
    APIError::Conflict(f!(
      "A file named {:?} already exists in folder with id {:?}",
      file.name,
      file.folder_id
    ))
  })
}
