#![cfg(test)]
mod files;

use format as f;

use crate::console::Colorize;
use crate::db::files::system::FileSystem;
use crate::db::files::{File, FileMetadata, Video};
use crate::db::Database;
use crate::{log, GracefulExit};

use mongodb::bson::doc;

pub const USER_ID1: &str = "google@test1";
pub const USER_ID2: &str = "google@test2";

pub async fn get_database() -> (FileSystem, Database) {
  let database = Database::new("test")
    .await
    .unwrap_or_exit("Could not create database");
  (FileSystem::from(&database), database)
}

pub async fn cleanup_files_collection(database: &Database) {
  log!(info@"Cleaning up files collection");
  let deleted_count = database
    .delete_many::<File>(doc! { "_id": { "$nin": [USER_ID1, USER_ID2] } })
    .await
    .unwrap_or_exit("Failed to cleanup files collection");
  log!(success@"Removed {deleted_count} documents from files collection");
}

#[derive(Clone)]
pub struct NestedFolderOptions<'a> {
  pub depth: usize,
  pub prefix: &'a str,
  pub parent_id: &'a str,
}

impl<'a> Default for NestedFolderOptions<'a> {
  fn default() -> Self {
    Self {
      depth: 3,
      prefix: "Folder",
      parent_id: "root",
    }
  }
}

pub async fn create_nested_folders<'a>(
  database: &'a Database,
  options: Option<NestedFolderOptions<'a>>,
) -> Vec<String> {
  let NestedFolderOptions {
    depth,
    prefix,
    parent_id,
  } = options.unwrap_or_default();
  let files = (0..depth)
    .map(|i| {
      create_folder_with_custom_id(
        f!("{prefix}-{i}"),
        USER_ID1.into(),
        f!("{prefix} {i}"),
        Some(if i > 0 {
          f!("{prefix}-{}", i - 1)
        } else {
          parent_id.to_string()
        }),
      )
    })
    .collect::<Vec<_>>();
  let ids = insert_many(database, files.as_slice()).await;
  ids
}

#[derive(Clone)]
pub struct FillFolderOptions<'a> {
  count: usize,
  prefix: &'a str,
  parent_id: &'a str,
}

impl<'a> Default for FillFolderOptions<'a> {
  fn default() -> Self {
    Self {
      count: 5,
      prefix: "File",
      parent_id: "root",
    }
  }
}

pub async fn fill_folder<'a>(
  database: &'a Database,
  options: Option<FillFolderOptions<'a>>,
) -> Vec<String> {
  let FillFolderOptions {
    count,
    prefix,
    parent_id,
  } = options.unwrap_or_default();
  let files = (0..count)
    .map(|i| {
      File::from_video(
        Video::default(),
        USER_ID1.into(),
        Some(parent_id.to_string()),
        Some(f!("{prefix} {i}")),
      )
      .unwrap_or_exit(f!("Could not create folder {prefix}-{i}"))
    })
    .collect::<Vec<_>>();
  let ids = insert_many(database, files.as_slice()).await;
  ids
}

/// - FolderOne 0
///   - FileOne 0
///   - FileOne 1
///   - FileOne 2
///   - FileOne 3
///   - FolderOne 1
///     - FolderOne 2
///       - FolderOne 3
///       - FolderThree 0
///         - FolderThree 1
///     - FolderTwo 0
///       - FolderTwo 1
///         - FolderTwo 2
///           - FolderTwo 3
///             - FolderTwo 4
pub async fn create_dummy_folder_structure(
  database: &Database,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
  let options = NestedFolderOptions {
    prefix: "FolderOne",
    depth: 4,
    ..Default::default()
  };
  let ids_one = create_nested_folders(database, Some(options)).await;
  let options = NestedFolderOptions {
    prefix: "FolderTwo",
    depth: 5,
    parent_id: &ids_one[1],
  };
  let ids_two = create_nested_folders(database, Some(options)).await;
  let options = NestedFolderOptions {
    prefix: "FolderThree",
    depth: 2,
    parent_id: &ids_one[2],
  };
  let ids_three = create_nested_folders(database, Some(options)).await;
  let options = FillFolderOptions {
    prefix: "FileOne",
    count: 4,
    parent_id: &ids_one[0],
  };
  let files = fill_folder(database, Some(options)).await;
  (ids_one, ids_two, ids_three, files)
}

pub async fn insert_many(database: &Database, files: &[File]) -> Vec<String> {
  let inserted_ids = database
    .create_many(files)
    .await
    .unwrap_or_exit("create_many database call failed")
    .into_values()
    .map(|id| {
      id.as_str()
        .map(String::from)
        .unwrap_or_else(|| id.to_string())
    })
    .collect::<Vec<_>>();

  files
    .iter()
    .filter_map(|file| {
      inserted_ids.contains(&file.id).then_some(file.id.clone())
    })
    .collect()
}

pub fn create_folder_with_custom_id(
  id: String,
  user_id: String,
  name: String,
  folder_id: Option<String>,
) -> File {
  File {
    id,
    folder_id: folder_id
      .map(|folder_id| File::map_folder_id(&user_id, &folder_id).to_string())
      .unwrap_or_else(|| user_id.clone()),
    user_id,
    name: name.try_into().unwrap_or_default(),
    metadata: FileMetadata::Folder,
  }
}
