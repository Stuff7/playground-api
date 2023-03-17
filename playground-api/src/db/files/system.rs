use std::{collections::HashSet, ops::Deref};

use mongodb::{
  bson::{doc, to_document},
  options::ReturnDocument,
  results::UpdateResult,
};
use thiserror::Error;

use crate::{
  db::{files::PartialFile, DBResult, Database},
  string::{NonEmptyString, StringError},
};

use super::{queries::FolderChange, File};

#[derive(Debug, Clone)]
pub struct FileSystem(pub Database);

impl Deref for FileSystem {
  type Target = Database;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl FileSystem {
  pub async fn move_many(
    &self,
    user_id: &str,
    files: &HashSet<String>,
    folder: &str,
  ) -> FileSystemResult<(UpdateResult, Option<Vec<FolderChange>>)> {
    if files.contains(user_id) {
      return Err(FileSystemError::ReadOnly);
    }
    let folder = File::map_folder_id(user_id, folder);
    if files.contains(folder) {
      return Err(FileSystemError::FolderLoop);
    }
    let query_result = self.get_many_children(user_id, files).await?;
    if let Some(ref result) = query_result {
      if result.children.contains(folder) {
        return Err(FileSystemError::FolderLoop);
      }
    }

    let result = self
      .update_many::<File>(
        doc! {
          File::folder_id(): folder,
        },
        self.query_many_by_id(user_id, files)?,
      )
      .await?;

    if result.modified_count > 0 {
      let mut folder_ids =
        query_result.map(|q| q.folders).unwrap_or_else(HashSet::new);
      folder_ids.insert(folder.to_string());
      let query = self.query_many_by_id(user_id, &folder_ids)?;
      let changes = self.lookup_folder_files(&query).await?;

      return Ok((result, Some(changes)));
    }
    Ok((result, None))
  }

  pub async fn update_one(
    &self,
    user_id: &str,
    file_id: &str,
    folder: Option<String>,
    name: Option<String>,
  ) -> FileSystemResult<(File, Vec<FolderChange>)> {
    if file_id == user_id {
      return Err(FileSystemError::ReadOnly);
    }
    let folder = folder.map(|f| File::map_folder_id(user_id, &f).to_string());
    if let Some(ref folder) = folder {
      let query_result = self.get_folder_children(user_id, file_id).await?;
      if let Some(ref result) = query_result {
        if result.children.contains(folder) {
          return Err(FileSystemError::FolderLoop);
        }
      }
    }
    let update = &mut PartialFile::default();
    update.name = name.map(NonEmptyString::try_from).transpose()?;
    update.folder_id = folder.clone();
    let update = self.query(update)?;
    let query = self.query(&PartialFile {
      id: Some(file_id.to_string()),
      user_id: Some(user_id.to_string()),
      ..Default::default()
    })?;
    let original_file = self
      .update::<File>(update, query, Some(ReturnDocument::Before))
      .await?
      .ok_or(FileSystemError::NotFound)?;
    let changes = if let Some(folder) = folder {
      let mut ids = HashSet::new();
      ids.insert(folder);
      ids.insert(original_file.folder_id.clone());
      self
        .lookup_folder_files(&self.query_many_by_id(user_id, &ids)?)
        .await?
    } else {
      self
        .lookup_folder_files(&self.query(&PartialFile {
          id: Some(original_file.folder_id.clone()),
          ..Default::default()
        })?)
        .await?
    };

    println!("CHANGES => {changes:#?}");

    Ok((original_file, changes))
  }

  pub async fn create_one(
    &self,
    user_file: &File,
  ) -> FileSystemResult<(File, Vec<FolderChange>)> {
    let new_file = self.save_one(user_file).await?.ok_or_else(|| {
      FileSystemError::NameConflict(
        user_file.name.clone(),
        user_file.folder_id.clone(),
      )
    })?;

    let query = self.query(&PartialFile {
      id: Some(new_file.folder_id.clone()),
      ..Default::default()
    })?;
    let changes = self.lookup_folder_files(&query).await?;

    Ok((new_file.clone(), changes))
  }

  pub async fn save_one(&self, file: &File) -> DBResult<Option<File>> {
    let mut query = &mut PartialFile::default();
    query.user_id = Some(file.user_id.clone());
    query.folder_id = Some(file.folder_id.clone());
    query.name = Some(file.name.clone());
    self.create(file, Some(to_document(query)?)).await
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
