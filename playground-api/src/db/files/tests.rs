#![cfg(test)]
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;

use format as f;

use crate::db::files::system::FileSystemError;
use crate::db::files::{File, ROOT_FOLDER_ALIAS};
use crate::db::init;
use crate::GracefulExit;

const USER_ID: &str = "google@cntascell";
static LOCK_DB: OnceCell<Mutex<bool>> = OnceCell::new();
static INITIALIZING: OnceCell<bool> = OnceCell::new();

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
    .unwrap_or_exit("");
  return *LOCK_DB.get().expect("LOCK_DB should be set").lock().await;
}

#[tokio::test]
async fn it_fails_to_move_folder_inside_itself() {
  if lock_database().await {
    let mut ids = Vec::new();
    for i in 0..3 {
      let (new_file, _) = File::create_one(
        &File::new_folder(
          USER_ID.into(),
          f!("Folder {i}"),
          ids.get(if i > 0 { i - 1 } else { 0 }).cloned(),
        )
        .unwrap_or_exit("Could not create folder"),
      )
      .await
      .unwrap_or_exit("Could not create folder");
      ids.push(new_file.id);
    }

    use FileSystemError::FolderLoop;
    let folder_to_move = &ids[0];
    for destination in &ids[1..] {
      let result = File::move_many(
        USER_ID,
        &mut vec![folder_to_move.to_string()].into_iter().collect(),
        destination,
      )
      .await;
      assert!(
        matches!(result, Err(FolderLoop)),
        "Expected moving {folder_to_move:?} to {destination:?} to fail with {FolderLoop}, instead got {result:#?}"
      );
    }
  }
}

#[tokio::test]
async fn it_fails_to_move_root_folder() {
  if lock_database().await {
    use FileSystemError::ReadOnly;
    for id in [USER_ID, ROOT_FOLDER_ALIAS] {
      let result = File::move_many(
        id,
        &mut vec![id.to_string()].into_iter().collect(),
        id,
      )
      .await;
      assert!(
        matches!(result, Err(ReadOnly)),
        "Expected moving {id:?} to fail with {ReadOnly}, instead got {result:#?}"
      );
    }
  }
}

// TODO: test happy path
#[tokio::test]
async fn it_moves_files() {
  if lock_database().await {
    use FileSystemError::ReadOnly;
    for id in [USER_ID, ROOT_FOLDER_ALIAS] {
      let result = File::move_many(
        id,
        &mut vec![id.to_string()].into_iter().collect(),
        id,
      )
      .await;
      assert!(
        matches!(result, Err(ReadOnly)),
        "Expected moving {id:?} to fail with {ReadOnly}, instead got {result:#?}"
      );
    }
  }
}
