#![cfg(test)]
use super::{
  cleanup_files_collection, create_dummy_folder_structure,
  create_nested_folders, get_database, NestedFolderOptions, USER_ID1,
};
use crate::{
  db::files::{system::FileSystemError, ROOT_FOLDER_ALIAS},
  GracefulExit,
};
use format as f;
use std::collections::HashSet;

#[tokio::test]
async fn it_fails_to_move_folder_inside_itself() {
  let (file_sys, database) = get_database().await;
  let ids = create_nested_folders(&database, None).await;

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
  let ids = create_nested_folders(&database, None).await;
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
  for change in changes {
    if change.folder_id == USER_ID1 {
      let files = change
        .children
        .iter()
        .map(|file| file.id.clone())
        .collect::<HashSet<_>>();
      assert!(
        files.eq(&ids_set),
        "Expected folder change to be {ids_set:?}, instead got {files:?}"
      );
      continue;
    }
    if &change.folder_id == id1 || &change.folder_id == id2 {
      assert!(
        change.children.is_empty(),
        "Expected folder change to be empty, instead got {:?}",
        change.children
      );
      continue;
    }
  }
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
  let (ids_one, ids_two, ids_three, file_ids) =
    create_dummy_folder_structure(&database).await;
  let ids_set =
    vec![file_ids[0].clone(), ids_one[3].clone(), ids_two[2].clone()]
      .into_iter()
      .collect();
  let (deleted_count, changes) = file_sys
    .delete_many(USER_ID1, &ids_set)
    .await
    .unwrap_or_exit("Failed to delete files");
  cleanup_files_collection(&database).await;
  assert!(
    deleted_count == 5,
    "Expected to delete 4 files, instead deleted {deleted_count}"
  );
  let change_count = changes.len();
  assert!(
    change_count == 3,
    "Expected 3 changes, instead got {change_count}"
  );
  let change_one = ids_one[2].clone();
  let change_two = ids_two[1].clone();
  let change_three = ids_one[0].clone();
  assert!(
    changes.iter().any(|c| c.folder_id == change_one),
    "Expected folder changes to include change one {change_one}",
  );
  assert!(
    changes.iter().any(|c| c.folder_id == change_two),
    "Expected folder changes to include change two {change_two}",
  );
  assert!(
    changes.iter().any(|c| c.folder_id == change_three),
    "Expected folder changes to include change three {change_three}",
  );
  for change in changes {
    let files = change
      .children
      .iter()
      .map(|file| file.id.clone())
      .collect::<HashSet<_>>();
    if change.folder_id == change_one {
      let set = vec![ids_three[0].clone()]
        .iter()
        .map(String::from)
        .collect();
      assert!(
        files.eq(&set),
        "Expected folder change to be {set:?}, instead got {files:?}"
      );
      continue;
    }
    if change.folder_id == change_two {
      assert!(
        files.is_empty(),
        "Expected folder change to be empty, instead got {files:?}"
      );
      continue;
    }
    if change.folder_id == change_three {
      let set = vec![
        file_ids[1].clone(),
        file_ids[2].clone(),
        file_ids[3].clone(),
        ids_one[1].clone(),
      ]
      .iter()
      .map(String::from)
      .collect();
      assert!(
        files.eq(&set),
        "Expected folder change to be {set:?}, instead got {files:?}"
      );
      continue;
    }
  }
}

#[tokio::test]
async fn it_fails_to_update_root_folder() {
  let (file_sys, ..) = get_database().await;
  use FileSystemError::ReadOnly;
  let result = file_sys
    .update_one(
      USER_ID1,
      USER_ID1,
      Some("new-folder-id".into()),
      Some("New Name".into()),
    )
    .await;
  assert!(
    matches!(result, Err(ReadOnly)),
    "Expected updating {USER_ID1:?} to fail with {ReadOnly}, instead got {result:#?}"
  );
}

#[tokio::test]
async fn it_fails_to_update_folder_to_be_inside_itself() {
  let (file_sys, database) = get_database().await;
  let ids = create_nested_folders(&database, None).await;
  use FileSystemError::FolderLoop;
  let result = file_sys
    .update_one(
      USER_ID1,
      &ids[1],
      Some(ids[2].clone()),
      Some("New Name".into()),
    )
    .await;
  cleanup_files_collection(&database).await;
  assert!(
    matches!(result, Err(FolderLoop)),
    "Expected updating {USER_ID1:?} to fail with {FolderLoop}, instead got {result:#?}"
  );
}

#[tokio::test]
async fn it_updates_file_successfully() {
  let (file_sys, database) = get_database().await;
  let ids = create_nested_folders(&database, None).await;
  let new_folders = create_nested_folders(
    &database,
    Some(NestedFolderOptions {
      prefix: "New Folder",
      ..Default::default()
    }),
  )
  .await;
  for (i, id) in ids.iter().enumerate() {
    let new_folder = new_folders[i].clone();
    let (_, changes) = file_sys
      .update_one(
        USER_ID1,
        id,
        Some(new_folder.clone()),
        Some(f!("New Name {i}")),
      )
      .await
      .unwrap_or_exit(f!("Expected file #{i} {id} update to succeed"));
    let change_count = changes.len();
    assert!(
      change_count == 2,
      "Expected to have 2 folder changes, instead got {change_count}"
    );
    let mut ids_set = HashSet::new();
    if let Some(id) = new_folders.get(i + 1).cloned() {
      ids_set.insert(id);
    }
    let old_folder = if i != 0 {
      ids[i - 1].clone()
    } else {
      USER_ID1.to_string()
    };
    for change in changes {
      let files = change
        .children
        .iter()
        .map(|file| file.id.clone())
        .collect::<HashSet<_>>();
      if change.folder_id == USER_ID1 {
        assert!(
          files.eq(&vec![new_folders[0].clone()].into_iter().collect()),
          "Expected folder change to be {ids_set:?}, instead got {files:?}"
        );
        continue;
      }
      if change.folder_id == old_folder {
        assert!(
          files.is_empty(),
          "Expected folder change to be empty, instead got {files:?}"
        );
        continue;
      }
      if change.folder_id == new_folder {
        ids_set.insert(id.clone());
        assert!(
          files.eq(&ids_set),
          "Expected folder change to be {ids_set:?}, instead got {files:?}"
        );
      }
    }
  }
  cleanup_files_collection(&database).await;
}
