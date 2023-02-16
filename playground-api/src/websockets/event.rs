use std::collections::HashSet;

use axum::extract::ws::Message;

use crate::{console::Colorize, log};

use super::channel::{
  EventMessage, EventReceiver, EventSender, SocketMessage, SocketSender,
};

pub enum Event {
  Add(EventType),
  Remove(EventExitRequest),
}

#[derive(Debug, Clone)]
pub struct EventExitRequest {
  pub socket_id: String,
  pub event_type: EventType,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum EventType {
  FileChange,
}

impl EventType {
  pub fn new(name: &str) -> Option<Self> {
    match name {
      "file-change" => Some(Self::FileChange),
      _ => None,
    }
  }
}

impl Event {
  pub fn new(message: &str, socket_id: String) -> Option<Self> {
    if !message.starts_with(EVENT_IDENTIFIER) {
      return None;
    }
    let mut parts = message[EVENT_IDENTIFIER.len()..].splitn(2, ':');
    let Some(action) = parts.next() else {return None};
    let Some(name) = parts.next() else {return None};
    let Some(event_type) = EventType::new(name) else {return None};

    match action {
      "add" => Some(Event::Add(event_type)),
      "remove" => Some(Event::Remove(EventExitRequest {
        socket_id,
        event_type,
      })),
      _ => None,
    }
  }
}

#[derive(Debug, Default)]
pub struct EventManager {
  events: HashSet<EventType>,
}

impl EventManager {
  pub fn process_event(
    &mut self,
    message: &str,
    socket_sender: &SocketSender,
    event_sender: &EventSender,
    user_id: String,
    socket_id: String,
  ) {
    let Some(event) = &Event::new(message, socket_id.clone()) else {return};
    match event {
      Event::Add(event_type) => {
        if self.events.contains(event_type) {
          log!(info@">>> {socket_id} Ignoring file-change event add request since is already added.");
          return;
        }
        match event_type {
          EventType::FileChange => {
            let mut socket_sender = socket_sender.clone();
            let mut event_receiver = event_sender.subscribe();
            log!(info@">>> {socket_id} Adding file-change event for {user_id:?}");
            tokio::spawn(async move {
              file_change_event_dispatcher(
                &mut socket_sender,
                &mut event_receiver,
                &user_id,
                &socket_id,
              )
              .await;
            });
            self.events.insert(event_type.clone());
          }
        }
      }
      Event::Remove(exit_request) => {
        if let Some(event_type) = self.events.take(&exit_request.event_type) {
          if let Err(error) =
            event_sender.send(EventMessage::Exit(exit_request.clone()))
          {
            log!(err@">>> {socket_id} Failed to remove event {event_type:?}: {error}");
          }
        }
      }
    }
  }
}

async fn file_change_event_dispatcher(
  socket_sender: &mut SocketSender,
  event_receiver: &mut EventReceiver,
  user_id: &str,
  socket_id: &str,
) {
  while let Ok(event) = event_receiver.recv().await {
    match event {
      EventMessage::Exit(EventExitRequest {
        event_type,
        socket_id: id,
      }) => {
        if id == socket_id && event_type == EventType::FileChange {
          log!(info@">>> {socket_id} exiting file-change event task");
          return;
        }
        log!(info@">>> {socket_id} file-change event received exit for {id} which is not us so we ignore");
        continue;
      }
      EventMessage::FileChange(change) => {
        if change.user_id != user_id {
          continue;
        }
        let Ok(json) = serde_json::to_string(&change) else {return};
        let message = SocketMessage::Message(Message::Text(json));
        if let Err(error) = socket_sender.send(message) {
          log!(err@">>> {socket_id} Could not send server message {change:#?}: {error}");
          return;
        }
      }
    }
  }
}

const EVENT_IDENTIFIER: &str = "event:";
