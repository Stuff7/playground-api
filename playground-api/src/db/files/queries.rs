use std::collections::HashSet;

use format as f;

use futures::TryStreamExt;
use mongodb::bson::{doc, to_bson, to_document, Document};
use partial_struct::{omit_and_create, CamelFields};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{
  system::FileSystem, Collection, DBResult, File, FileMetadata, PartialFile,
};

#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct FolderChange {
  pub user_id: String,
  pub folder_id: String,
  pub files: Vec<File>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct FileIds {
  pub ids: HashSet<String>,
  pub folder_ids: HashSet<String>,
}

#[omit_and_create(FolderFamilyMember)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderFamily {
  #[serde(rename = "_id")]
  pub id: String,
  pub folder_id: String,
  pub name: String,
  #[omit]
  pub parents: Vec<FolderFamilyMember>,
  #[omit]
  pub children: Vec<FolderFamilyMember>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderChildren {
  #[serde(rename = "_id")]
  pub id: String,
  pub user_id: String,
  pub metadata: FileMetadata,
  pub children: HashSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManyChildrenQueryResult {
  pub children: HashSet<String>,
  pub folders: HashSet<String>,
}

impl FileSystem {
  /// Finds nested files for the given ids and splits them into a set of _id's and a set of folder_id's
  pub async fn query_nested_files(
    &self,
    user_id: &str,
    ids: &HashSet<String>,
  ) -> DBResult<Option<FileIds>> {
    let query = &to_bson::<HashSet<String>>(ids)?;
    let pipeline = vec![
      doc! { "$match": {
        "$or": [
          { "_id": { "$in": query } },
          { File::folder_id(): { "$in": query } }
        ],
        File::user_id(): user_id
      } },
      query_all_children(),
      doc! { "$project": {
        "dupedIds": {
          "$concatArrays": [["$_id"], "$children._id"]
        },
        "dupedFolderIds": {
          "$concatArrays": [[f!("${}", File::folder_id())], f!("$children.{}", File::folder_id())]
        },
      } },
      doc! { "$unwind": "$dupedIds" },
      doc! { "$unwind": "$dupedFolderIds" },
      doc! { "$group": {
        "_id": null,
        "ids": {
          "$addToSet": "$dupedIds"
        },
        "folderIds": {
          "$addToSet": "$dupedFolderIds"
        }
      } },
      doc! { "$project": {
        "_id": 0,
        "ids": "$ids",
        "folderIds": "$folderIds",
      } },
    ];

    Ok(self.aggregate::<FileIds>(pipeline).await?.pop())
  }

  pub async fn lookup_folder_files(
    &self,
    query: &Document,
  ) -> DBResult<Vec<FolderChange>> {
    let pipeline = vec![
      doc! { "$match": query },
      query_direct_children(),
      doc! { "$project": {
        "_id": 0,
        File::folder_id(): "$_id",
        File::user_id(): 1,
        File::collection_name(): "$directChildren"
      }},
    ];

    self.aggregate::<FolderChange>(pipeline).await
  }

  pub async fn get_folder_family(
    &self,
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<FolderFamily>> {
    let pipeline = vec![
      doc! { "$match": query_by_id(user_id, folder_id)? },
      query_all_parents(),
      query_direct_children(),
      doc! { "$project": {
        "_id": 1,
        File::name(): 1,
        File::folder_id(): 1,
        "parents._id": 1,
        f!("parents.{}", File::name()): 1,
        f!("parents.{}", File::folder_id()): 1,
        "children._id": "directChildren._id",
        f!("children.{}", File::name()): f!("directChildren.{}", File::name()),
        f!("children.{}", File::folder_id()): f!("directChildren.{}", File::name()),
      } },
    ];

    Ok(self.aggregate::<FolderFamily>(pipeline).await?.pop().map(
      |mut family| {
        family
          .parents
          .sort_by_key(|d| (d.id.clone(), d.folder_id.clone()));
        family
      },
    ))
  }

  pub async fn get_folder_children(
    &self,
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<FolderChildren>> {
    let pipeline = vec![
      doc! { "$match": query_by_id(user_id, folder_id)? },
      query_all_children(),
      doc! { "$project": {
        "_id": 1,
        "metadata": 1,
        File::user_id(): 1,
        "children": "$children._id",
      } },
    ];

    Ok(self.aggregate::<FolderChildren>(pipeline).await?.pop())
  }

  // Gets all children for any of the `files` and their direct folders
  pub async fn get_many_children(
    &self,
    user_id: &str,
    files: &HashSet<String>,
  ) -> DBResult<Option<ManyChildrenQueryResult>> {
    let pipeline = vec![
      doc! { "$match": query_many_by_id(user_id, files)? },
      query_all_children(),
      doc! { "$addFields": { "children": { "$cond": {
        "if": { "$eq": [ { "$size": "$children" }, 0 ] },
        "then": [null],
        "else": "$children"
      } } } },
      doc! { "$unwind": "$children" },
      doc! { "$group": {
        "_id": null,
        "children": { "$addToSet": "$children._id" },
        "folders": { "$addToSet": f!("${}", File::folder_id()) },
      } },
      doc! { "$project": {
        "_id": 0,
        "children": 1,
        "folders": 1,
      } },
    ];

    Ok(
      self
        .aggregate::<ManyChildrenQueryResult>(pipeline)
        .await?
        .pop(),
    )
  }

  async fn aggregate<T: DeserializeOwned + Unpin + Send + Sync>(
    &self,
    pipeline: impl IntoIterator<Item = Document>,
  ) -> DBResult<Vec<T>> {
    Ok(
      self
        .database
        .aggregate::<File>(pipeline)
        .await?
        .with_type::<T>()
        .try_collect::<Vec<T>>()
        .await?,
    )
  }
}

fn query_all_children() -> Document {
  doc! { "$graphLookup": {
    "from": File::collection_name(),
    "startWith": "$_id",
    "connectFromField": "_id",
    "connectToField": File::folder_id(),
    "as": "children",
    "maxDepth": 99,
  } }
}

fn query_all_parents() -> Document {
  doc! { "$graphLookup": {
    "from": File::collection_name(),
    "startWith": f!("${}", File::folder_id()),
    "connectFromField": File::folder_id(),
    "connectToField": "_id",
    "as": "parents",
    "maxDepth": 99,
    "restrictSearchWithMatch": { "metadata.type": "folder" }
  } }
}

fn query_direct_children() -> Document {
  doc! { "$lookup": {
    "from": File::collection_name(),
    "pipeline": [
      { "$addFields": {
        "insensitiveName": { "$toLower": f!("${}", File::name()) },
      } },
      { "$sort": { "insensitiveName": 1 } },
      { "$project": { "insensitiveName": 0 } }
    ],
    "localField": "_id",
    "foreignField": File::folder_id(),
    "as": "directChildren",
  } }
}

pub fn query_by_file(file: &PartialFile) -> DBResult<Document> {
  Ok(to_document::<PartialFile>(file)?)
}

pub fn query_by_id(user_id: &str, id: &str) -> DBResult<Document> {
  Ok(doc! { File::user_id(): user_id, "_id": File::map_folder_id(user_id, id) })
}

pub fn query_many_by_id(
  user_id: &str,
  ids: &HashSet<String>,
) -> DBResult<Document> {
  Ok(
    doc! { File::user_id(): user_id, "_id": { "$in": to_bson::<HashSet<String>>(ids)? } },
  )
}
