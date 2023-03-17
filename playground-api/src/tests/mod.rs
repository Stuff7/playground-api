#![cfg(test)]
use std::collections::HashSet;

use format as f;

use crate::console::Colorize;
use crate::db::files::system::{FileSystem, FileSystemError};
use crate::db::files::{File, ROOT_FOLDER_ALIAS};
use crate::db::Database;
use crate::{log, GracefulExit};

use mongodb::bson::doc;

const USER_ID1: &str = "google@test1";
const USER_ID2: &str = "google@test2";

#[tokio::test]
async fn it_fails_to_move_folder_inside_itself() {
  let (file_sys, database) = get_database().await;
  let ids = create_nested_folders(&file_sys, None).await;

  use FileSystemError::FolderLoop;
  let folder_to_move = &ids[0];
  for destination in &ids[1..] {
    let result = file_sys
      .move_many(
        USER_ID1,
        &vec![folder_to_move.to_string()].into_iter().collect(),
        destination,
      )
      .await;
    assert!(
      matches!(result, Err(FolderLoop)),
      "Expected moving {folder_to_move:?} to {destination:?} to fail with {FolderLoop}, instead got {result:#?}"
    );
  }
  cleanup_files_collection(&database).await;
}

#[tokio::test]
async fn it_fails_to_move_root_folder() {
  let (file_sys, ..) = get_database().await;
  use FileSystemError::ReadOnly;
  for id in [USER_ID1, ROOT_FOLDER_ALIAS] {
    let result = file_sys
      .move_many(id, &vec![id.to_string()].into_iter().collect(), id)
      .await;
    assert!(
      matches!(result, Err(ReadOnly)),
      "Expected moving {id:?} to fail with {ReadOnly}, instead got {result:#?}"
    );
  }
}

#[tokio::test]
async fn it_moves_files_successfully() {
  let (file_sys, database) = get_database().await;
  let ids = create_nested_folders(&file_sys, None).await;
  let ids_set = ids.clone().into_iter().collect();
  let (result, changes) = file_sys
    .move_many(USER_ID1, &ids_set, ROOT_FOLDER_ALIAS)
    .await
    .unwrap_or_exit("Failed to move files to root folder");
  let moved_count = result.modified_count;
  cleanup_files_collection(&database).await;
  assert!(
    moved_count == 2,
    "Expected to move 2 files, instead moved {moved_count}"
  );
  let changes = changes.expect("There should be changes");
  let [id1, id2, ..] = &ids[..] else {
    unreachable!("There should be more than 2 ids, but there were not. {ids:#?}");
  };
  for id in [USER_ID1, id1, id2] {
    assert!(
      changes.iter().any(|change| change.folder_id == id),
      "Expected changes to contain {id} but it did not.\n\nChanges => {changes:#?}"
    );
  }
  assert!(
    changes.iter().all(|change| {
      if change.folder_id == USER_ID1 {
        let files = change
          .files
          .iter()
          .map(|file| file.id.clone())
          .collect::<HashSet<_>>();
        return files.eq(&ids_set);
      }
      if &change.folder_id == id1 || &change.folder_id == id2 {
        return change.files.is_empty();
      }
      false
    }),
    "Folder Changes does not contain correct files.\n\n Changes => {changes:#?}"
  );
}

#[tokio::test]
async fn it_fails_to_delete_root_folder() {
  let (file_sys, ..) = get_database().await;
  use FileSystemError::ReadOnly;
  for id in [USER_ID1, ROOT_FOLDER_ALIAS] {
    let result = file_sys
      .delete_many(id, &vec![id.to_string()].into_iter().collect())
      .await;
    assert!(
      matches!(result, Err(ReadOnly)),
      "Expected deleting {id:?} to fail with {ReadOnly}, instead got {result:#?}"
    );
  }
}

#[tokio::test]
async fn it_deletes_files_successfully() {
  let (file_sys, database) = get_database().await;
  let (ids_one, ids_two, ids_three) =
    create_dummy_folder_structure(&file_sys).await;
  let ids_set = vec![ids_one[3].clone(), ids_two[2].clone()]
    .into_iter()
    .collect();
  let (deleted_count, changes) = file_sys
    .delete_many(USER_ID1, &ids_set)
    .await
    .unwrap_or_exit("Failed to delete files");
  cleanup_files_collection(&database).await;
  assert!(
    deleted_count == 4,
    "Expected to delete 4 files, instead deleted {deleted_count}"
  );
  let change_count = changes.len();
  assert!(
    change_count == 2,
    "Expected 2 changes, instead got {change_count}"
  );
  let change_one = ids_one[2].clone();
  let change_two = ids_two[1].clone();
  assert!(
    changes.iter().any(|c| c.folder_id == change_one),
    "Expected folder changes to include change one {change_one}",
  );
  assert!(
    changes.iter().any(|c| c.folder_id == change_two),
    "Expected folder changes to include change two {change_two}",
  );
  assert!(
    changes.iter().all(|change| {
      let files = change
        .files
        .iter()
        .map(|file| file.id.clone())
        .collect::<HashSet<_>>();
      if change.folder_id == change_one {
        let set = vec![ids_three[0].clone()].iter().map(String::from).collect();
        return files.eq(&set);
      }
      if change.folder_id == change_two {
        return files.eq(&HashSet::new());
      }
      false
    }),
    "Folder Changes does not contain correct files.\n\n Changes => {changes:#?}"
  );
}

async fn get_database() -> (FileSystem, Database) {
  let database = Database::new("test")
    .await
    .unwrap_or_exit("Could not create database");
  (FileSystem::from(&database), database)
}

async fn cleanup_files_collection(database: &Database) {
  log!(info@"Cleaning up files collection");
  let deleted_count = database
    .delete_many::<File>(doc! { "_id": { "$nin": [USER_ID1, USER_ID2] } })
    .await
    .unwrap_or_exit("Failed to cleanup files collection");
  log!(success@"Removed {deleted_count} documents from files collection");
}

#[derive(Clone)]
struct NestedFolderOptions<'a> {
  depth: usize,
  prefix: &'a str,
  parent_id: &'a str,
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

async fn create_nested_folders<'a>(
  file_system: &'a FileSystem,
  options: Option<NestedFolderOptions<'a>>,
) -> Vec<String> {
  let mut ids = Vec::new();
  let NestedFolderOptions {
    depth,
    prefix,
    parent_id,
  } = options.unwrap_or_default();
  for i in 0..depth {
    let (new_file, ..) = file_system
      .create_one(
        &File::new_folder(
          USER_ID1.into(),
          f!("{prefix} {i}"),
          ids
            .get(if i > 0 { i - 1 } else { 0 })
            .cloned()
            .or_else(|| Some(parent_id.to_string())),
        )
        .unwrap_or_exit("Could not create folder"),
      )
      .await
      .unwrap_or_exit("Could not save folder");
    ids.push(new_file.id);
  }
  ids
}

/// - FolderOne 0
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
async fn create_dummy_folder_structure(
  file_system: &FileSystem,
) -> (Vec<String>, Vec<String>, Vec<String>) {
  let options = NestedFolderOptions {
    prefix: "FolderOne",
    depth: 4,
    ..Default::default()
  };
  let ids_one = create_nested_folders(file_system, Some(options.clone())).await;
  let options = NestedFolderOptions {
    prefix: "FolderTwo",
    depth: 5,
    parent_id: &ids_one[1],
  };
  let ids_two = create_nested_folders(file_system, Some(options)).await;
  let options = NestedFolderOptions {
    prefix: "FolderThree",
    depth: 2,
    parent_id: &ids_one[2],
  };
  let ids_three = create_nested_folders(file_system, Some(options)).await;
  (ids_one, ids_two, ids_three)
}
