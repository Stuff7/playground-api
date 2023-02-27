use std::collections::HashSet;

use crate::{
  api::{self, APIError, APIResult},
  auth::session::{FileId, FileIdVecQuery, FolderBody, Session},
  console::Colorize,
  db::{self, FolderChange},
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

pub async fn get_files(
  query: db::PartialUserFile,
) -> APIResult<Json<Vec<db::UserFile>>> {
  let files = db::DATABASE
    .find_many::<db::UserFile>(db::UserFile::query(&query)?)
    .await
    .unwrap_or_default();
  Ok(Json(files))
}

pub async fn get_folder_family(
  session: Session,
  Path(folder_id): Path<String>,
) -> APIResult<Json<db::FolderFamily>> {
  Ok(Json(
    db::UserFile::get_folder_family(&session.user_id, &folder_id)
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
) -> APIResult<Json<db::UserFile>> {
  let mut metadata = fetch_video_metadata(&request_client, &video_id).await?;

  if let Some(thumbnail) = body.thumbnail {
    metadata.thumbnail = thumbnail;
  }

  save_file(
    &db::UserFile::from_video(metadata, session.user_id, folder, body.name)?,
    event_sender,
  )
  .await
  .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderBody {
  name: String,
}

pub async fn create_folder(
  session: Session,
  State(event_sender): State<EventSender>,
  FolderBody(folder, body): FolderBody<CreateFolderBody>,
) -> APIResult<Json<db::UserFile>> {
  save_file(
    &db::UserFile::new_folder(session.user_id, body.name, folder)?,
    event_sender,
  )
  .await
  .map(Json)
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

// Things to notify on:
// * The files from the folders of the moved files
// * The files from the destination folder
pub async fn move_files(
  session: Session,
  State(event_sender): State<EventSender>,
  FolderBody(folder, mut body): FolderBody<MoveFilesBody>,
) -> APIResult<Json<MoveFilesResponse>> {
  let folder = folder.unwrap_or_else(|| session.user_id.clone());
  body.files.remove(&session.user_id);
  body.files.remove(&folder);
  let update = db::UserFile::query(&db::PartialUserFile {
    folder_id: Some(folder.clone()),
    ..Default::default()
  })?;

  let query = db::UserFile::query_many_by_id(&session.user_id, &body.files)?;

  let mut folder_ids = db::DATABASE
    .find_many::<db::UserFile>(query.clone())
    .await?
    .into_iter()
    .map(|file| file.folder_id)
    .collect::<HashSet<_>>();

  let result = db::DATABASE
    .update_many::<db::UserFile>(update, query)
    .await?;

  if result.modified_count > 0 {
    folder_ids.insert(folder);
    let query = db::UserFile::query_many_by_id(&session.user_id, &folder_ids)?;
    let changes = db::UserFile::lookup_folder_files(&query, false).await?;

    send_folder_changes(&event_sender, changes)?;
  }

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
  State(event_sender): State<EventSender>,
  FileId(file_id): FileId,
  FolderBody(folder, body): FolderBody<UpdateFileBody>,
) -> APIResult<Json<db::UserFile>> {
  let update = &mut db::PartialUserFile::default();
  update.name = body.name.map(db::NonEmptyString::try_from).transpose()?;
  update.folder_id = folder.clone();
  let update = db::UserFile::query(update)?;
  let query = db::UserFile::query(&db::PartialUserFile {
    id: Some(file_id.clone()),
    user_id: Some(session.user_id.clone()),
    ..Default::default()
  })?;
  let original_file = db::DATABASE
    .update::<db::UserFile>(update, query, Some(db::ReturnDocument::Before))
    .await?
    .ok_or_else(|| {
      APIError::NotFound(f!("File with id {file_id:?} not found"))
    })?;
  let changes = if folder.is_some() {
    db::UserFile::lookup_folder_files(
      &db::UserFile::query_many(
        &session.user_id,
        &vec![
          db::PartialUserFile {
            folder_id: folder,
            ..Default::default()
          },
          db::PartialUserFile {
            folder_id: Some(original_file.folder_id.clone()),
            ..Default::default()
          },
        ],
      )?,
      true,
    )
    .await?
  } else {
    db::UserFile::lookup_folder_files(
      &db::UserFile::query(&db::PartialUserFile {
        id: Some(original_file.id.clone()),
        ..Default::default()
      })?,
      true,
    )
    .await?
  };

  send_folder_changes(&event_sender, changes)?;

  Ok(Json(original_file))
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
    db::UserFile::query_nested_files(&session.user_id, &query).await? else {
      return Ok(Json(DeleteFilesResponse { deleted: 0 }))
    };

  let query = db::UserFile::query_many_by_id(&session.user_id, &result.ids)?;
  let folder_query =
    db::UserFile::query_many_by_id(&session.user_id, &result.folder_ids)?;
  let deleted = db::DATABASE.delete_many::<db::UserFile>(query).await?;
  let changes = db::UserFile::lookup_folder_files(&folder_query, false).await?;

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
) -> APIResult<Json<db::Video>> {
  Ok(Json(
    fetch_video_metadata(&request_client, &video_id).await?,
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

async fn save_file(
  user_file: &db::UserFile,
  event_sender: EventSender,
) -> APIResult<db::UserFile> {
  let new_file = db::save_file(user_file).await?.ok_or_else(|| {
    APIError::Conflict(f!(
      "A file named {:?} already exists in folder with id {:?}",
      user_file.name,
      user_file.folder_id
    ))
  })?;

  let query = db::UserFile::query(&db::PartialUserFile {
    folder_id: Some(new_file.folder_id.clone()),
    ..Default::default()
  })?;
  let changes = db::UserFile::lookup_folder_files(&query, true).await?;

  send_folder_changes(&event_sender, changes)?;

  Ok(new_file.clone())
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
