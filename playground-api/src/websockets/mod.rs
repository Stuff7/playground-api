mod channel;
mod file_watcher;

use crate::{auth::session::SessionQuery, console::Colorize, db::DBError, log};

use std::borrow::Cow;
use std::net::SocketAddr;
use std::ops::ControlFlow;

use axum::{
  extract::{
    connect_info::ConnectInfo,
    ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade},
    State, TypedHeader,
  },
  headers,
  response::IntoResponse,
  routing::get,
  Router,
};

use futures::{sink::SinkExt, stream::StreamExt};

use thiserror::Error;

use self::{
  channel::{EventChannel, EventMessage, EventSender},
  file_watcher::FileWatcher,
};

#[derive(Debug, Clone)]
pub struct WebSocketState {
  event_sender: EventSender,
}

pub fn api() -> Router {
  let event_channel = EventChannel::new();
  FileWatcher::new(event_channel.sender.clone());
  Router::new()
    .route("/", get(ws_handler))
    .with_state(WebSocketState {
      event_sender: event_channel.sender,
    })
}

/// The handler for the HTTP request (this gets called when the HTTP GET lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
async fn ws_handler(
  ws: WebSocketUpgrade,
  user_agent: Option<TypedHeader<headers::UserAgent>>,
  SessionQuery(session): SessionQuery,
  ConnectInfo(addr): ConnectInfo<SocketAddr>,
  State(state): State<WebSocketState>,
) -> impl IntoResponse {
  let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
    user_agent.to_string()
  } else {
    String::from("Unknown browser")
  };
  log!(success@"`{user_agent}` at {addr} connected.");

  ws.on_upgrade(move |socket| {
    handle_socket(socket, addr, session.user_id, state.event_sender)
  })
}

/// WebSocket state machine (one will be spawned per connection)
async fn handle_socket(
  mut socket: WebSocket,
  socket_id: SocketAddr,
  user_id: String,
  event_sender: EventSender,
) {
  if let Err(error) = socket.send(Message::Ping(vec![1, 2, 3])).await {
    log!(err@"Could not send ping to {socket_id}.\n\nError: {error}");
    return;
  }

  log!(success@"Pinged {socket_id}...");

  let (mut socket_sender, mut socket_receiver) = socket.split();

  // Task to notify the client of any updates in the files collection
  let mut event_receiver = event_sender.subscribe();
  let event_user_id = user_id.clone();
  let mut send_task = tokio::spawn(async move {
    while let Ok(event) = event_receiver.recv().await {
      match event {
        EventMessage::Exit(user_id) => {
          if user_id == event_user_id {
            log!(info@"Files listener task received exit signal, exiting...");
            return;
          }
          log!(info@"Exit received for {user_id} but id is {event_user_id} so we ignore");
          continue;
        }
        EventMessage::FileChange(change) => {
          if change.user_id != event_user_id {
            continue;
          }
          let Ok(json) = serde_json::to_string(&change) else {return};
          if let Err(error) = socket_sender.send(Message::Text(json)).await {
            log!(err@"Could not send server message {change:#?}.\n\nError: {error}");
            return;
          }
        }
      }
    }
    log!("Closing connection: {socket_id}...");
    if let Err(error) = socket_sender
      .send(Message::Close(Some(CloseFrame {
        code: close_code::NORMAL,
        reason: Cow::from("Goodbye"),
      })))
      .await
    {
      log!(err@"Could not close connection {socket_id}: {error}");
    }
  });

  // Task to receive messages from the client and log them to the console
  let mut recv_task = tokio::spawn(async move {
    let mut count = 0;
    while let Some(Ok(msg)) = socket_receiver.next().await {
      count += 1;
      if process_message(msg, socket_id).is_break() {
        break;
      }
    }
    count
  });

  // If any one of the tasks exits, send a signal to the other to exit too.
  tokio::select! {
    rv_a = (&mut send_task) => {
      match rv_a {
        Ok(_) => log!(success@"Sent messages to {socket_id}"),
        Err(a) => log!(err@"Error sending messages {a:?}")
      }
    },
    rv_b = (&mut recv_task) => {
      match rv_b {
        Ok(b) => log!(success@"Received {b} messages"),
        Err(b) => log!(err@"Error receiving messages {b:?}")
      }
      if let Err(error) = event_sender.send(EventMessage::Exit(user_id)) {
        log!(err@"Error sending exit from message receiver task\n\nError:{error}");
      }
    }
  }

  // Returning from the handler closes the websocket connection
  log!(success@"Websocket context {socket_id} destroyed");
}

fn process_message(msg: Message, socket_id: SocketAddr) -> ControlFlow<(), ()> {
  match msg {
    Message::Text(t) => {
      log!(">>> {socket_id} sent str: {t:?}");
    }
    Message::Binary(d) => {
      log!(">>> {socket_id} sent {} bytes: {d:?}", d.len());
    }
    Message::Close(c) => {
      if let Some(cf) = c {
        log!(
          ">>> {socket_id} sent close with code {} and reason `{}`",
          cf.code,
          cf.reason
        );
      } else {
        log!(">>> {socket_id} somehow sent close message without CloseFrame");
      }
      return ControlFlow::Break(());
    }

    Message::Pong(v) => {
      log!(">>> {socket_id} sent pong with {v:?}");
    }
    // No need to manually handle Message::Ping. But we can access the pings content here.
    Message::Ping(v) => {
      log!(">>> {socket_id} sent ping with {v:?}");
    }
  }
  ControlFlow::Continue(())
}

#[derive(Error, Debug)]
pub enum WebSocketError {
  #[error("A JSON error occurred in a WebSocket: {0}")]
  Json(#[from] serde_json::Error),
  #[error("A database error occurred in a WebSocket: {0}")]
  Database(#[from] DBError),
}
