use super::{
  aggregations::FolderChildren,
  queries::{query_by_file, query_many_by_id},
  File,
};
use crate::{
  db::{files::PartialFile, DBResult, Database},
  string::{NonEmptyString, StringError},
};
use mongodb::{
  bson::{doc, to_document},
  options::ReturnDocument,
  results::UpdateResult,
};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct FileSystem {
  pub(super) database: Database,
}

impl From<&Database> for FileSystem {
  fn from(database: &Database) -> Self {
    Self {
      database: database.clone(),
    }
  }
}

impl FileSystem {
  pub async fn find_many(
    &self,
    query: &PartialFile,
  ) -> FileSystemResult<Vec<File>> {
    Ok(
      self
        .database
        .find_many::<File>(query_by_file(query)?)
        .await
        .unwrap_or_default(),
    )
  }

  pub async fn move_many(
    &self,
    user_id: &str,
    files: &HashSet<String>,
    folder: &str,
  ) -> FileSystemResult<(UpdateResult, Option<Vec<FolderChildren>>)> {
    if files.contains(user_id) {
      return Err(FileSystemError::ReadOnly);
    }
    let folder = File::map_folder_id(user_id, folder);
    if files.contains(folder) {
      return Err(FileSystemError::FolderLoop);
    }
    let query_result = self.find_lineage_and_parents(user_id, files).await?;
    if let Some(ref result) = query_result {
      if result.lineage.contains(folder) {
        return Err(FileSystemError::FolderLoop);
      }
    }

    let result = self
      .database
      .update_many::<File>(
        doc! {
          File::folder_id(): folder,
        },
        query_many_by_id(user_id, files)?,
      )
      .await?;

    if result.modified_count > 0 {
      let mut folder_ids = query_result.map(|q| q.parents).unwrap_or_default();
      folder_ids.insert(folder.to_string());
      let query = query_many_by_id(user_id, &folder_ids)?;
      let changes = self.find_folder_with_children(&query).await?;

      return Ok((result, Some(changes)));
    }
    Ok((result, None))
  }

  pub async fn delete_many(
    &self,
    user_id: &str,
    ids: &HashSet<String>,
  ) -> FileSystemResult<(u64, Vec<FolderChildren>)> {
    if ids.contains(user_id) {
      return Err(FileSystemError::ReadOnly);
    }
    // find nested files and it's parents
    let Some(result) = self.find_lineage_with_parents(user_id, ids).await? else {
      return Ok((0, Vec::new()))
    };

    let deleted = self
      .database
      .delete_many::<File>(query_many_by_id(user_id, &result.lineage)?)
      .await?;

    let changes = self
      .find_folder_with_children(&query_many_by_id(user_id, &result.parents)?)
      .await?;

    Ok((deleted, changes))
  }

  pub async fn update_one(
    &self,
    user_id: &str,
    file_id: &str,
    folder: Option<String>,
    name: Option<String>,
  ) -> FileSystemResult<(File, Vec<FolderChildren>)> {
    if file_id == user_id {
      return Err(FileSystemError::ReadOnly);
    }
    let folder = folder.map(|f| File::map_folder_id(user_id, &f).to_string());
    if let Some(ref folder) = folder {
      if let Some(lineage) = self.find_lineage(user_id, file_id).await? {
        if lineage.contains(folder) {
          return Err(FileSystemError::FolderLoop);
        }
      }
    }
    let update = &mut PartialFile::default();
    update.name = name.map(NonEmptyString::try_from).transpose()?;
    update.folder_id = folder.clone();
    let update = query_by_file(update)?;
    let query = query_by_file(&PartialFile {
      id: Some(file_id.to_string()),
      user_id: Some(user_id.to_string()),
      ..Default::default()
    })?;
    let original_file = self
      .database
      .update::<File>(update, query, Some(ReturnDocument::Before))
      .await?
      .ok_or(FileSystemError::NotFound)?;
    let changes = if let Some(folder) = folder {
      let mut ids = HashSet::new();
      ids.insert(folder);
      ids.insert(original_file.folder_id.clone());
      self
        .find_folder_with_children(&query_many_by_id(user_id, &ids)?)
        .await?
    } else {
      self
        .find_folder_with_children(&query_by_file(&PartialFile {
          id: Some(original_file.folder_id.clone()),
          ..Default::default()
        })?)
        .await?
    };

    Ok((original_file, changes))
  }

  pub async fn create_one(
    &self,
    user_file: &File,
  ) -> FileSystemResult<(File, Vec<FolderChildren>)> {
    let new_file = self.save_one(user_file).await?.ok_or_else(|| {
      FileSystemError::NameConflict(
        user_file.name.clone(),
        user_file.folder_id.clone(),
      )
    })?;

    let query = query_by_file(&PartialFile {
      id: Some(new_file.folder_id.clone()),
      ..Default::default()
    })?;
    let changes = self.find_folder_with_children(&query).await?;

    Ok((new_file.clone(), changes))
  }

  async fn save_one(&self, file: &File) -> DBResult<Option<File>> {
    let mut query = &mut PartialFile::default();
    query.user_id = Some(file.user_id.clone());
    query.folder_id = Some(file.folder_id.clone());
    query.name = Some(file.name.clone());
    self.database.create(file, Some(to_document(query)?)).await
  }
}

#[derive(Error, Debug)]
pub enum FileSystemError {
  #[error("A folder cannot contain itself")]
  FolderLoop,
  #[error("Root folder is read-only")]
  ReadOnly,
  #[error("File not found")]
  NotFound,
  #[error("Internal database error {0}")]
  Internal(#[from] super::super::DBError),
  #[error("Bad formatted string {0}")]
  BadString(#[from] StringError),
  #[error("A file with the name {0:?} already exists in folder with id {1:?}")]
  NameConflict(NonEmptyString, String),
}

pub type FileSystemResult<T = ()> = Result<T, FileSystemError>;
