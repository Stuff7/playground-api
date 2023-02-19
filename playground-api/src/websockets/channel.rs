use crate::db;

use super::event::EventExitRequest;

use axum::extract::ws::Message;
use tokio::sync::broadcast;

#[derive(Debug)]
pub struct BroadcastChannel<T: Clone> {
  pub sender: broadcast::Sender<T>,
  pub receiver: broadcast::Receiver<T>,
}

impl<T: Clone> BroadcastChannel<T> {
  pub fn new() -> Self {
    let (sender, receiver) = broadcast::channel(16);
    Self { sender, receiver }
  }
}

#[derive(Debug, Clone)]
pub enum EventMessage {
  FolderChange(db::FolderChange),
  Exit(EventExitRequest),
}

#[derive(Debug, Clone)]
pub enum SocketMessage {
  Message(Message),
  Exit,
}

pub type EventChannel = BroadcastChannel<EventMessage>;
pub type EventReceiver = broadcast::Receiver<EventMessage>;
pub type EventSender = broadcast::Sender<EventMessage>;
pub type EventSendError = broadcast::error::SendError<EventMessage>;

pub type SocketChannel = BroadcastChannel<SocketMessage>;
pub type SocketReceiver = broadcast::Receiver<SocketMessage>;
pub type SocketSender = broadcast::Sender<SocketMessage>;
