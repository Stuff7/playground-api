use std::collections::HashSet;

use format as f;

use futures::TryStreamExt;
use mongodb::bson::{doc, oid::ObjectId, to_bson, to_document, Document};
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::Mutex;

use partial_struct::{partial, CamelFields};

use super::DBResult;

pub static SESSIONS_CACHE: Lazy<Mutex<HashSet<String>>> =
  Lazy::new(|| Mutex::new(HashSet::new()));

pub trait Collection:
  std::fmt::Debug
  + Serialize
  + DeserializeOwned
  + Unpin
  + Send
  + Sync
  + Clone
  + 'static
{
  fn collection_name() -> &'static str;
  fn id(&self) -> &str;
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
}

#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct FolderChange {
  pub user_id: String,
  pub folder_id: String,
  pub files: Vec<UserFile>,
}

const ROOT_FOLDER_ALIAS: &str = "root";

#[partial]
#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct UserFile {
  #[serde(rename = "_id")]
  pub id: String,
  pub folder_id: String,
  pub user_id: String,
  pub name: NonEmptyString,
  pub metadata: FileMetadata,
}

impl Collection for UserFile {
  fn collection_name() -> &'static str {
    "files"
  }
  fn id(&self) -> &str {
    &self.id
  }
}

impl UserFile {
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

  pub fn folder_query(
    user_id: &str,
    folder_id: Option<String>,
  ) -> DBResult<Document> {
    let folder_id = folder_id
      .map(|id| to_bson(&Self::map_folder_id(user_id, &id)))
      .unwrap_or_else(|| to_bson(&doc!("$ne": ROOT_FOLDER_ALIAS)))?;

    Ok(doc! {
      Self::user_id(): user_id,
      Self::folder_id(): folder_id
    })
  }

  pub fn query(user_file: &PartialUserFile) -> DBResult<Document> {
    Ok(to_document::<PartialUserFile>(user_file)?)
  }

  pub fn query_many(
    user_id: &str,
    user_files: &Vec<PartialUserFile>,
  ) -> DBResult<Document> {
    Ok(
      doc! { Self::user_id(): user_id, "$or": to_bson::<Vec<PartialUserFile>>(user_files)? },
    )
  }

  /// Runs an aggregation that follows these stages:
  /// * Match all the files using the `filter` + `user_id`
  /// * Lookups all the files that share a `folder_id` with any of the previous matched files
  /// * Groups all the matches by their `folder_id`
  /// * Returns the groups as a `Vec<FolderChange>`
  pub async fn get_folder_files(
    query: &Document,
  ) -> DBResult<Vec<FolderChange>> {
    let pipeline = vec![
      doc! { "$match": query },
      doc! { "$lookup": {
          "from": Self::collection_name(),
          "localField": Self::folder_id(),
          "foreignField": Self::folder_id(),
          "as": Self::collection_name()
      }},
      doc! { "$unwind": f!("${}", Self::collection_name()) },
      doc! { "$replaceRoot": { "newRoot": f!("${}", Self::collection_name()) } },
      doc! { "$group": {
          "_id": {
            Self::folder_id(): f!("${}", Self::folder_id()),
            Self::user_id(): f!("${}", Self::user_id()),
            "file_id": "$_id"
          },
          "file": { "$first": "$$ROOT" }
      }},
      doc! { "$group": {
          "_id": f!("$_id.{}", Self::folder_id()),
          Self::user_id(): { "$first": f!("$_id.{}", Self::user_id()) },
          Self::collection_name(): { "$push": "$file" }
      }},
      doc! { "$project": {
          "_id": 0,
          Self::folder_id(): "$_id",
          Self::user_id(): 1,
          Self::collection_name(): 1
      }},
    ];
    let changes = super::DATABASE
      .aggregate::<Self>(pipeline)
      .await?
      .with_type::<FolderChange>()
      .try_collect::<Vec<_>>()
      .await?;

    Ok(changes)
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NonEmptyString(String);

impl NonEmptyString {
  fn try_from_str(s: &str) -> super::DBResult<Self> {
    if s.is_empty() {
      Err(super::DBError::InvalidField(
        "String cannot be empty".into(),
      ))
    } else {
      Ok(NonEmptyString(s.to_string()))
    }
  }
}

impl TryFrom<String> for NonEmptyString {
  type Error = super::DBError;

  fn try_from(s: String) -> super::DBResult<Self> {
    NonEmptyString::try_from_str(&s)
  }
}

impl TryFrom<&str> for NonEmptyString {
  type Error = super::DBError;

  fn try_from(s: &str) -> super::DBResult<Self> {
    NonEmptyString::try_from_str(s)
  }
}
