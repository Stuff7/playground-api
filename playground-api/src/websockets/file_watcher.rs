use futures::StreamExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task::JoinHandle;

use crate::{
  console::Colorize,
  db::{DBError, UserFile, DATABASE},
  log,
};

use super::channel::{EventMessage, EventSendError, EventSender};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileChange {
  pub user_id: String,
  pub folder_id: String,
  pub files: Vec<UserFile>,
}

pub struct FileWatcher(JoinHandle<u32>);

impl FileWatcher {
  pub fn new(sender: EventSender) -> Self {
    let listener_task = tokio::spawn(async move {
      match Self::listen(&sender).await {
        Err(error) => {
          log!(err@"There was an error listening for files {error}");
          0
        }
        Ok(sent_messages) => sent_messages,
      }
    });
    FileWatcher(listener_task)
  }

  pub async fn listen(sender: &EventSender) -> FileWatcherResult<u32> {
    log!(info@"Listening for user files changes");
    let mut change_stream = DATABASE.watch::<UserFile>().await?;
    let mut sent_msg_count = 0;

    while let Some(result) = change_stream.next().await {
      if let Some(file_change) = result.map_err(DBError::from)?.full_document {
        let files = DATABASE
          .find_many::<UserFile>(UserFile::folder_query(
            file_change.user_id.clone(),
            Some(file_change.folder_id.clone()),
          )?)
          .await
          .unwrap_or_default();

        log!(info@"File changed sending message...");
        sender.send(EventMessage::FileChange(FileChange {
          user_id: file_change.user_id,
          folder_id: file_change.folder_id,
          files,
        }))?;
        sent_msg_count += 1;
      }
    }
    Ok(sent_msg_count)
  }
}

#[derive(Error, Debug)]
pub enum FileWatcherError {
  #[error("Error sending event message: {0}")]
  Json(#[from] EventSendError),
  #[error("Database error when watching files: {0}")]
  Database(#[from] DBError),
}

pub type FileWatcherResult<T = ()> = Result<T, FileWatcherError>;
