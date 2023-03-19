use super::{
  queries::{
    query_ancestors, query_by_id, query_children, query_lineage,
    query_many_by_id,
  },
  system::FileSystem,
  BasicFileInfo, DBResult, File,
};
use format as f;
use futures::TryStreamExt;
use mongodb::bson::{doc, to_bson, Document};
use partial_struct::{omit_and_create, CamelFields};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashSet, ops::Deref};

#[derive(Debug, Serialize, Deserialize, Clone, CamelFields)]
#[serde(rename_all = "camelCase")]
pub struct FolderChildren {
  #[serde(flatten)]
  file: BasicFileInfo,
  pub children: Vec<File>,
}

impl Deref for FolderChildren {
  type Target = BasicFileInfo;
  fn deref(&self) -> &Self::Target {
    &self.file
  }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderChildrenAndAncestors {
  #[serde(flatten)]
  file: BasicFileInfo,
  pub ancestors: Vec<BasicFileInfo>,
  pub children: Vec<File>,
}

impl Deref for FolderChildrenAndAncestors {
  type Target = BasicFileInfo;
  fn deref(&self) -> &Self::Target {
    &self.file
  }
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
      query_lineage(),
      doc! { "$project": {
        "dupedIds": {
          "$concatArrays": [["$_id"], "$lineage._id"]
        },
        "dupedFolderIds": {
          "$concatArrays": [[f!("${}", File::folder_id())], f!("$lineage.{}", File::folder_id())]
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
  ) -> DBResult<Vec<FolderChildren>> {
    let pipeline = vec![doc! { "$match": query }, query_children()];

    self.aggregate::<FolderChildren>(pipeline).await
  }

  pub async fn find_children_and_ancestors(
    &self,
    user_id: &str,
    folder_id: &str,
  ) -> DBResult<Option<FolderChildrenAndAncestors>> {
    let pipeline = [doc! { "$match": query_by_id(user_id, folder_id)? }]
      .into_iter()
      .chain(query_ancestors())
      .chain([query_children()])
      .collect::<Vec<_>>();

    Ok(
      self
        .aggregate::<FolderChildrenAndAncestors>(pipeline)
        .await?
        .pop(),
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
          query_lineage(),
          doc! { "$project": { "_id": 0, "lineage": "$lineage._id", } },
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
      query_lineage(),
      doc! { "$addFields": { "lineage": { "$cond": {
        "if": { "$eq": [ { "$size": "$lineage" }, 0 ] },
        "then": [null],
        "else": "$lineage"
      } } } },
      doc! { "$unwind": "$lineage" },
      doc! { "$group": {
        "_id": null,
        "lineage": { "$addToSet": "$lineage._id" },
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
