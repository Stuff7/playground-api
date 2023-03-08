use std::collections::HashSet;

use format as f;

use futures::TryStreamExt;
use mongodb::bson::{doc, to_bson, to_document, Document};
use partial_struct::{omit_and_create, CamelFields};
use serde::{Deserialize, Serialize};

use super::{
  super::DATABASE, Collection, DBResult, File, FileMetadata, PartialFile,
  ROOT_FOLDER_ALIAS,
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

#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct DirectAndAllChildrenQueryResult {
  pub user_id: String,
  pub folder_id: String,
  pub all_children: HashSet<String>,
  pub direct_children: Vec<File>,
}

impl File {
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

  pub fn query(user_file: &PartialFile) -> DBResult<Document> {
    Ok(to_document::<PartialFile>(user_file)?)
  }

  pub fn query_many_by_id(
    user_id: &str,
    ids: &HashSet<String>,
  ) -> DBResult<Document> {
    Ok(
      doc! { Self::user_id(): user_id, "_id": { "$in": to_bson::<HashSet<String>>(ids)? } },
    )
  }

  pub fn query_many(
    user_id: &str,
    user_files: &Vec<PartialFile>,
  ) -> DBResult<Document> {
    Ok(
      doc! { Self::user_id(): user_id, "$or": to_bson::<Vec<PartialFile>>(user_files)? },
    )
  }

  /// Finds nested files for the given ids and splits them into a set of _id's and a set of folder_id's
  pub async fn query_nested_files(
    user_id: &str,
    ids: &HashSet<String>,
  ) -> DBResult<Option<FileIds>> {
    let query = &to_bson::<HashSet<String>>(ids)?;
    let pipeline = vec![
      doc! { "$match": {
        "$or": [
          { "_id": { "$in": query } },
          { Self::folder_id(): { "$in": query } }
        ],
        Self::user_id(): user_id
      } },
      Self::find_all_children_stage(),
      doc! { "$project": {
        "dupedIds": {
          "$concatArrays": [["$_id"], "$children._id"]
        },
        "dupedFolderIds": {
          "$concatArrays": [[f!("${}", Self::folder_id())], f!("$children.{}", Self::folder_id())]
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

    let result = DATABASE
      .collection::<Self>()
      .aggregate(pipeline, None)
      .await?
      .with_type::<FileIds>()
      .try_collect::<Vec<FileIds>>()
      .await?;
    let result = result.into_iter().next();

    Ok(result)
  }

  pub async fn lookup_folder_files(
    query: &Document,
  ) -> DBResult<Vec<FolderChange>> {
    let pipeline = vec![
      doc! { "$match": query },
      Self::find_direct_children_stage(),
      doc! { "$project": {
        "_id": 0,
        Self::folder_id(): "$_id",
        Self::user_id(): 1,
        Self::collection_name(): "$directChildren"
      }},
    ];
    let changes = DATABASE
      .aggregate::<Self>(pipeline)
      .await?
      .with_type::<FolderChange>()
      .try_collect::<Vec<_>>()
      .await?;

    Ok(changes)
  }

  pub async fn get_direct_and_all_children(
    user_id: &str,
    files: &HashSet<String>,
  ) -> DBResult<Vec<DirectAndAllChildrenQueryResult>> {
    let pipeline = vec![
      doc! { "$match": {
        "_id": { "$in": to_bson::<HashSet<String>>(files)? },
        Self::user_id(): user_id,
      } },
      Self::find_all_children_stage(),
      Self::find_direct_children_stage(),
      doc! { "$project": {
        "_id": 0,
        Self::folder_id(): "$_id",
        Self::user_id(): 1,
        "allChildren": "$children._id",
        "directChildren": 1,
      }},
    ];
    let changes = DATABASE
      .aggregate::<Self>(pipeline)
      .await?
      .with_type::<DirectAndAllChildrenQueryResult>()
      .try_collect::<Vec<_>>()
      .await?;

    Ok(changes)
  }

  pub async fn get_folder_family(
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<FolderFamily>> {
    let pipeline = vec![
      doc! { "$match": {
        "_id": Self::map_folder_id(user_id, folder_id),
        Self::user_id(): user_id
      } },
      Self::find_all_parents_stage(),
      Self::find_direct_children_stage(),
      doc! { "$project": {
        "_id": 1,
        Self::name(): 1,
        Self::folder_id(): 1,
        "parents._id": 1,
        f!("parents.{}", Self::name()): 1,
        f!("parents.{}", Self::folder_id()): 1,
        "children._id": "directChildren._id",
        f!("children.{}", Self::name()): f!("directChildren.{}", Self::name()),
        f!("children.{}", Self::folder_id()): f!("directChildren.{}", Self::name()),
      } },
    ];

    let family = DATABASE
      .aggregate::<Self>(pipeline)
      .await?
      .with_type::<FolderFamily>()
      .try_next()
      .await?
      .map(|mut family| {
        family
          .parents
          .sort_by_key(|d| (d.id.clone(), d.folder_id.clone()));
        family
      });

    Ok(family)
  }

  pub async fn get_folder_children(
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<FolderChildren>> {
    let pipeline = vec![
      doc! { "$match": {
        "_id": folder_id,
        Self::user_id(): user_id,
      } },
      Self::find_all_children_stage(),
      doc! { "$project": {
        "_id": 1,
        "metadata": 1,
        Self::user_id(): 1,
        "children": "$children._id",
      } },
    ];

    Ok(
      DATABASE
        .aggregate::<Self>(pipeline)
        .await?
        .with_type::<FolderChildren>()
        .try_next()
        .await?,
    )
  }

  // Gets all children for any of the `files` and their direct folders
  pub async fn get_many_children(
    user_id: &str,
    files: &HashSet<String>,
  ) -> DBResult<Option<ManyChildrenQueryResult>> {
    let pipeline = vec![
      doc! { "$match": {
        "_id": { "$in": to_bson::<HashSet<String>>(files)? },
        Self::user_id(): user_id,
      } },
      Self::find_all_children_stage(),
      doc! { "$unwind": "$children" },
      doc! { "$group": {
        "_id": null,
        "children": { "$addToSet": "$children._id" },
        "folders": { "$addToSet": f!("${}", Self::folder_id()) },
      } },
      doc! { "$project": {
        "_id": 0,
        "children": 1,
        "folders": 1,
      } },
    ];

    Ok(
      DATABASE
        .aggregate::<Self>(pipeline)
        .await?
        .with_type::<ManyChildrenQueryResult>()
        .try_next()
        .await?,
    )
  }

  fn find_all_children_stage() -> Document {
    doc! { "$graphLookup": {
      "from": Self::collection_name(),
      "startWith": "$_id",
      "connectFromField": "_id",
      "connectToField": Self::folder_id(),
      "as": "children",
      "maxDepth": 99,
    } }
  }

  fn find_all_parents_stage() -> Document {
    doc! { "$graphLookup": {
      "from": Self::collection_name(),
      "startWith": f!("${}", Self::folder_id()),
      "connectFromField": Self::folder_id(),
      "connectToField": "_id",
      "as": "parents",
      "maxDepth": 99,
      "restrictSearchWithMatch": { "metadata.type": "folder" }
    } }
  }

  fn find_direct_children_stage() -> Document {
    doc! { "$lookup": {
      "from": Self::collection_name(),
      "pipeline": [
        { "$addFields": {
          "insensitiveName": { "$toLower": f!("${}", Self::name()) },
        } },
        { "$sort": { "insensitiveName": 1 } },
        { "$project": { "insensitiveName": 0 } }
      ],
      "localField": "_id",
      "foreignField": Self::folder_id(),
      "as": "directChildren",
    } }
  }
}
