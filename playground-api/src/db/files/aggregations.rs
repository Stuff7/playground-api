use super::{
  queries::{
    query_all_children, query_all_parents, query_by_id, query_direct_children,
    query_many_by_id,
  },
  system::FileSystem,
  DBResult, File,
};
use format as f;
use futures::TryStreamExt;
use mongodb::bson::{doc, to_bson, Document};
use partial_struct::{omit_and_create, CamelFields};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct FolderWithChildren {
  pub user_id: String,
  pub folder_id: String,
  pub children: Vec<File>,
}

#[omit_and_create(FolderFamilyMember)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderChildrenAndAncestors {
  #[serde(rename = "_id")]
  pub id: String,
  pub folder_id: String,
  pub name: String,
  #[omit]
  pub ancestors: Vec<FolderFamilyMember>,
  #[omit]
  pub children: Vec<FolderFamilyMember>,
}

#[omit_and_create(Lineage)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageAndParents {
  pub lineage: HashSet<String>,
  #[omit]
  pub parents: HashSet<String>,
}

impl FileSystem {
  /// Returns all children for the given `ids` and the direct parents of those children
  pub async fn find_lineage_with_parents(
    &self,
    user_id: &str,
    ids: &HashSet<String>,
  ) -> DBResult<Option<LineageAndParents>> {
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
        "lineage": "$ids",
        "parents": "$folderIds",
      } },
    ];

    Ok(self.aggregate::<LineageAndParents>(pipeline).await?.pop())
  }

  pub async fn find_folder_with_children(
    &self,
    query: &Document,
  ) -> DBResult<Vec<FolderWithChildren>> {
    let pipeline = vec![
      doc! { "$match": query },
      query_direct_children(),
      doc! { "$project": {
        "_id": 0,
        File::folder_id(): "$_id",
        File::user_id(): 1,
        "children": "$directChildren"
      }},
    ];

    self.aggregate::<FolderWithChildren>(pipeline).await
  }

  pub async fn find_children_and_ancestors(
    &self,
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<FolderChildrenAndAncestors>> {
    let pipeline = vec![
      doc! { "$match": query_by_id(user_id, folder_id)? },
      query_all_parents(),
      query_direct_children(),
      doc! { "$project": {
        "_id": 1,
        File::name(): 1,
        File::folder_id(): 1,
        "ancestors._id": "parents._id",
        f!("ancestors.{}", File::name()): f!("parents.{}", File::name()),
        f!("ancestors.{}", File::folder_id()): f!("parents.{}", File::folder_id()),
        "children._id": "directChildren._id",
        f!("children.{}", File::name()): f!("directChildren.{}", File::name()),
        f!("children.{}", File::folder_id()): f!("directChildren.{}", File::folder_id()),
      } },
    ];

    Ok(
      self
        .aggregate::<FolderChildrenAndAncestors>(pipeline)
        .await?
        .pop()
        .map(|mut family| {
          family
            .ancestors
            .sort_by_key(|d| (d.id.clone(), d.folder_id.clone()));
          family
        }),
    )
  }

  pub async fn find_lineage(
    &self,
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<HashSet<String>>> {
    Ok(
      self
        .aggregate::<Lineage>(vec![
          doc! { "$match": query_by_id(user_id, folder_id)? },
          query_all_children(),
          doc! { "$project": { "_id": 0, "lineage": "$children._id", } },
        ])
        .await?
        .pop()
        .map(|Lineage { lineage }| lineage),
    )
  }

  pub async fn find_lineage_and_parents(
    &self,
    user_id: &str,
    files: &HashSet<String>,
  ) -> DBResult<Option<LineageAndParents>> {
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
        "lineage": { "$addToSet": "$children._id" },
        "parents": { "$addToSet": f!("${}", File::folder_id()) },
      } },
      doc! { "$project": {
        "_id": 0,
        "lineage": 1,
        "parents": 1,
      } },
    ];

    Ok(self.aggregate::<LineageAndParents>(pipeline).await?.pop())
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
