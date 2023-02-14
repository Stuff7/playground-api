use super::file_watcher::FileChange;

use tokio::sync::broadcast;

#[derive(Debug)]
pub struct EventChannel {
  pub sender: EventSender,
  pub receiver: EventReceiver,
}

impl EventChannel {
  pub fn new() -> Self {
    let (sender, receiver) = broadcast::channel(16);
    Self { sender, receiver }
  }
}

#[derive(Debug, Clone)]
pub enum EventMessage {
  FileChange(FileChange),
  Exit(String),
}

pub type EventReceiver = broadcast::Receiver<EventMessage>;
pub type EventSender = broadcast::Sender<EventMessage>;
pub type EventSendError = broadcast::error::SendError<EventMessage>;
