use std::collections::{HashMap, HashSet};

use mongodb::bson::{doc, oid::ObjectId, to_bson, to_document, Document};
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::Mutex;

use partial_struct::{partial, CamelFields};

use super::DBResult;

pub type Cache<T> = Lazy<Mutex<HashMap<String, T>>>;

pub static SESSIONS_CACHE: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
pub static USERS_CACHE: Cache<User> = Lazy::new(|| Mutex::new(HashMap::new()));
pub static USER_FILES_CACHE: Cache<UserFile> = Lazy::new(|| Mutex::new(HashMap::new()));

pub trait Collection:
  std::fmt::Debug + Serialize + DeserializeOwned + Unpin + Send + Sync + Clone + 'static
{
  fn collection_name() -> &'static str;
  fn id(&self) -> &str;
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionCache {
  _id: String,
  pub sessions: HashSet<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct User {
  #[serde(rename = "_id")]
  pub _id: String,
  pub name: String,
  pub picture: String,
}

impl User {
  pub fn new(id: &str, name: &str, picture: &str) -> Self {
    Self {
      _id: id.to_string(),
      name: name.to_string(),
      picture: picture.to_string(),
    }
  }
}

impl Collection for User {
  fn collection_name() -> &'static str {
    "users"
  }
  fn id(&self) -> &str {
    &self._id
  }
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized,
  {
    &USERS_CACHE
  }
}

#[partial]
#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct UserFile {
  #[serde(rename = "_id")]
  pub id: String,
  pub folder_id: String,
  pub user_id: String,
  pub name: String,
  pub metadata: FileMetadata,
}

impl UserFile {
  pub fn from_video(
    video: Video,
    user_id: String,
    folder_id: Option<String>,
    custom_name: Option<String>,
  ) -> Self {
    Self {
      id: ObjectId::new().to_hex(),
      folder_id: folder_id.unwrap_or_else(|| user_id.clone()),
      user_id,
      name: custom_name.unwrap_or_else(|| video.name.clone()),
      metadata: FileMetadata::Video(video),
    }
  }

  pub fn new_folder(user_id: String, name: String, folder_id: Option<String>) -> Self {
    Self {
      id: ObjectId::new().to_hex(),
      folder_id: folder_id.unwrap_or_else(|| user_id.clone()),
      user_id,
      name,
      metadata: FileMetadata::Folder,
    }
  }

  pub fn new_root_folder(user_id: String) -> Self {
    Self {
      id: user_id.clone(),
      folder_id: "root".to_string(),
      user_id,
      name: "root".to_string(),
      metadata: FileMetadata::Folder,
    }
  }

  pub fn user_query(file_id: String, user_id: String) -> DBResult<Document> {
    let query = &mut PartialUserFile::default();
    query.id = Some(file_id);
    query.user_id = Some(user_id);
    Self::query(query)
  }

  pub fn folder_query(user_id: String, folder_id: Option<String>) -> DBResult<Document> {
    let mut query = doc! {
      UserFile::user_id(): user_id
    };
    query.insert(
      UserFile::folder_id(),
      match folder_id {
        Some(folder_id) => to_bson(&folder_id)?,
        None => to_bson(&doc!("$ne": "root"))?,
      },
    );

    Ok(query)
  }

  pub fn update_query(name: Option<String>, folder_id: Option<String>) -> DBResult<Document> {
    let update = &mut PartialUserFile::default();
    update.name = name;
    update.folder_id = folder_id;
    Self::query(update)
  }

  pub fn files_query(user_id: String, files: &HashSet<String>) -> DBResult<Document> {
    let query = &mut PartialUserFile::default();
    query.user_id = Some(user_id);
    let mut query = Self::query(query)?;
    let files = files
      .iter()
      .map(|id| PartialUserFile {
        id: Some(id.to_string()),
        ..Default::default()
      })
      .collect::<Vec<_>>();
    query.insert("$or", to_bson::<Vec<PartialUserFile>>(&files)?);
    Ok(query)
  }

  pub fn query(user_file: &PartialUserFile) -> DBResult<Document> {
    Ok(to_document::<PartialUserFile>(user_file)?)
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FileMetadata {
  Video(Video),
  Folder,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Video {
  pub name: String,
  pub play_id: String,
  pub duration_millis: u64,
  pub width: u16,
  pub height: u16,
  pub thumbnail: String,
  pub mime_type: String,
  pub size_bytes: u64,
}

impl Collection for UserFile {
  fn collection_name() -> &'static str {
    "files"
  }
  fn id(&self) -> &str {
    &self.id
  }
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized,
  {
    &USER_FILES_CACHE
  }
}