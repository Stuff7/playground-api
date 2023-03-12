use std::collections::HashSet;

use mongodb::{bson::doc, options::ReturnDocument, results::UpdateResult};
use thiserror::Error;

use crate::{
  db::{files::PartialFile, save_file},
  string::{NonEmptyString, StringError},
};

use super::{super::DATABASE, queries::FolderChange, File};

impl File {
  pub async fn move_many(
    user_id: &str,
    files: &HashSet<String>,
    folder: &str,
  ) -> FileSystemResult<(UpdateResult, Option<Vec<FolderChange>>)> {
    if files.contains(user_id) {
      return Err(FileSystemError::ReadOnly);
    }
    let folder = Self::map_folder_id(user_id, folder);
    if files.contains(folder) {
      return Err(FileSystemError::FolderLoop);
    }
    let query_result = Self::get_many_children(user_id, files).await?;
    if let Some(ref result) = query_result {
      if result.children.contains(folder) {
        return Err(FileSystemError::FolderLoop);
      }
    }

    let result = DATABASE
      .update_many::<Self>(
        doc! {
          Self::folder_id(): folder,
        },
        Self::query_many_by_id(user_id, files)?,
      )
      .await?;

    if result.modified_count > 0 {
      let mut folder_ids =
        query_result.map(|q| q.folders).unwrap_or_else(HashSet::new);
      folder_ids.insert(folder.to_string());
      let query = Self::query_many_by_id(user_id, &folder_ids)?;
      let changes = Self::lookup_folder_files(&query).await?;

      return Ok((result, Some(changes)));
    }
    Ok((result, None))
  }

  pub async fn update_one(
    user_id: &str,
    file_id: &str,
    folder: Option<String>,
    name: Option<String>,
  ) -> FileSystemResult<(File, Vec<FolderChange>)> {
    if file_id == user_id {
      return Err(FileSystemError::ReadOnly);
    }
    let folder = folder.map(|f| Self::map_folder_id(user_id, &f).to_string());
    if let Some(ref folder) = folder {
      let query_result = Self::get_folder_children(user_id, file_id).await?;
      if let Some(ref result) = query_result {
        if result.children.contains(folder) {
          return Err(FileSystemError::FolderLoop);
        }
      }
    }
    let update = &mut PartialFile::default();
    update.name = name.map(NonEmptyString::try_from).transpose()?;
    update.folder_id = folder.clone();
    let update = File::query(update)?;
    let query = File::query(&PartialFile {
      id: Some(file_id.to_string()),
      user_id: Some(user_id.to_string()),
      ..Default::default()
    })?;
    let original_file = DATABASE
      .update::<File>(update, query, Some(ReturnDocument::Before))
      .await?
      .ok_or(FileSystemError::NotFound)?;
    let changes = if let Some(folder) = folder {
      let mut ids = HashSet::new();
      ids.insert(folder);
      ids.insert(original_file.folder_id.clone());
      File::lookup_folder_files(&File::query_many_by_id(user_id, &ids)?).await?
    } else {
      File::lookup_folder_files(&File::query(&PartialFile {
        id: Some(original_file.folder_id.clone()),
        ..Default::default()
      })?)
      .await?
    };

    println!("CHANGES => {changes:#?}");

    Ok((original_file, changes))
  }

  pub async fn create_one(
    user_file: &Self,
  ) -> FileSystemResult<(Self, Vec<FolderChange>)> {
    let new_file = save_file(user_file).await?.ok_or_else(|| {
      FileSystemError::NameConflict(
        user_file.name.clone(),
        user_file.folder_id.clone(),
      )
    })?;

    let query = Self::query(&PartialFile {
      id: Some(new_file.folder_id.clone()),
      ..Default::default()
    })?;
    let changes = Self::lookup_folder_files(&query).await?;

    Ok((new_file.clone(), changes))
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
