#![cfg(test)]
use std::collections::HashSet;

use format as f;

use crate::console::Colorize;
use crate::db::files::system::FileSystemError;
use crate::db::files::{File, ROOT_FOLDER_ALIAS};
use crate::db::{init, DATABASE};
use crate::{log, GracefulExit};

use mongodb::bson::doc;
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;

const USER_ID1: &str = "google@test1";
const USER_ID2: &str = "google@test2";
static LOCK_DB: OnceCell<Mutex<bool>> = OnceCell::new();
static INITIALIZING: OnceCell<bool> = OnceCell::new();

#[tokio::test]
async fn it_fails_to_move_folder_inside_itself() {
  if lock_database().await {
    let ids = create_nested_folders().await;

    use FileSystemError::FolderLoop;
    let folder_to_move = &ids[0];
    for destination in &ids[1..] {
      let result = File::move_many(
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
    cleanup_files_collection().await;
  }
}

#[tokio::test]
async fn it_fails_to_move_root_folder() {
  if lock_database().await {
    use FileSystemError::ReadOnly;
    for id in [USER_ID1, ROOT_FOLDER_ALIAS] {
      let result =
        File::move_many(id, &vec![id.to_string()].into_iter().collect(), id)
          .await;
      assert!(
        matches!(result, Err(ReadOnly)),
        "Expected moving {id:?} to fail with {ReadOnly}, instead got {result:#?}"
      );
    }
  }
}

#[tokio::test]
async fn it_moves_files_successfully() {
  if lock_database().await {
    let ids = create_nested_folders().await;
    let ids_set = ids.clone().into_iter().collect();
    let (result, changes) =
      File::move_many(USER_ID1, &ids_set, ROOT_FOLDER_ALIAS)
        .await
        .unwrap_or_exit("Failed to move files to root folder");
    let moved_count = result.modified_count;
    cleanup_files_collection().await;
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
}

async fn poll_db() -> bool {
  loop {
    let Some(db) = LOCK_DB.get() else {
      continue;
    };
    return *db.lock().await;
  }
}

async fn lock_database() -> bool {
  if *INITIALIZING.get().unwrap_or(&false) {
    return poll_db().await;
  }
  if let Some(db) = LOCK_DB.get() {
    return *db.lock().await;
  }
  if INITIALIZING.set(true).is_err() {
    return poll_db().await;
  }
  init("test").await;
  LOCK_DB
    .set(Mutex::new(true))
    .map_err(|_| "")
    .unwrap_or_exit("Tried to initialize database more than once");
  return *LOCK_DB.get().expect("LOCK_DB should be set").lock().await;
}

async fn cleanup_files_collection() {
  log!(info@"Cleaning up files collection");
  let deleted_count = DATABASE
    .delete_many::<File>(doc! { "_id": { "$nin": [USER_ID1, USER_ID2] } })
    .await
    .unwrap_or_exit("Failed to cleanup files collection");
  log!(success@"Removed {deleted_count} documents from files collection");
}

async fn create_nested_folders() -> Vec<String> {
  let mut ids = Vec::new();
  for i in 0..3 {
    let (new_file, _) = File::create_one(
      &File::new_folder(
        USER_ID1.into(),
        f!("Folder {i}"),
        ids.get(if i > 0 { i - 1 } else { 0 }).cloned(),
      )
      .unwrap_or_exit("Could not create folder"),
    )
    .await
    .unwrap_or_exit("Could not save folder");
    ids.push(new_file.id);
  }
  ids
}
