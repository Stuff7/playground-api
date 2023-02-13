use super::file_watcher::FileChange;

use tokio::sync::broadcast;

#[derive(Debug)]
pub struct SocketChannel {
  pub sender: Sender,
  pub receiver: Receiver,
}

impl SocketChannel {
  pub fn new() -> Self {
    let (sender, receiver) = broadcast::channel(16);
    Self { sender, receiver }
  }
}

#[derive(Debug, Clone)]
pub enum SocketEvent {
  FileChange(FileChange),
  Exit(String),
}

pub type Receiver = broadcast::Receiver<SocketEvent>;
pub type Sender = broadcast::Sender<SocketEvent>;
pub type SendError = broadcast::error::SendError<SocketEvent>;
