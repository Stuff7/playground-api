pub mod queries;
pub mod system;
pub mod tests;

use crate::string::NonEmptyString;

use mongodb::bson::{doc, oid::ObjectId};
use serde::{Deserialize, Serialize};

use partial_struct::{partial, CamelFields};

use super::{Collection, DBResult};

pub const ROOT_FOLDER_ALIAS: &str = "root";

#[partial]
#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct File {
  #[serde(rename = "_id")]
  pub id: String,
  pub folder_id: String,
  pub user_id: String,
  pub name: NonEmptyString,
  pub metadata: FileMetadata,
}

impl Collection for File {
  fn collection_name() -> &'static str {
    "files"
  }
  fn id(&self) -> &str {
    &self.id
  }
}

impl File {
  pub fn from_video(
    video: Video,
    user_id: String,
    folder_id: Option<String>,
    custom_name: Option<String>,
  ) -> DBResult<Self> {
    Ok(Self {
      id: ObjectId::new().to_hex(),
      folder_id: folder_id.unwrap_or_else(|| user_id.clone()),
      user_id,
      name: custom_name
        .unwrap_or_else(|| video.name.clone())
        .try_into()?,
      metadata: FileMetadata::Video(video),
    })
  }

  pub fn new_folder(
    user_id: String,
    name: String,
    folder_id: Option<String>,
  ) -> DBResult<Self> {
    Ok(Self {
      id: ObjectId::new().to_hex(),
      folder_id: folder_id.unwrap_or_else(|| user_id.clone()),
      user_id,
      name: name.try_into()?,
      metadata: FileMetadata::Folder,
    })
  }

  pub fn new_root_folder(user_id: String) -> DBResult<Self> {
    Ok(Self {
      id: user_id.clone(),
      folder_id: ROOT_FOLDER_ALIAS.to_string(),
      user_id,
      name: ROOT_FOLDER_ALIAS.try_into()?,
      metadata: FileMetadata::Folder,
    })
  }

  pub fn map_folder_id<'a>(user_id: &'a str, folder_id: &'a str) -> &'a str {
    if folder_id == ROOT_FOLDER_ALIAS {
      user_id
    } else {
      folder_id
    }
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
