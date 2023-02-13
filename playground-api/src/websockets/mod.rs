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
  channel::{Sender, SocketChannel, SocketEvent},
  file_watcher::FileWatcher,
};

#[derive(Debug, Clone)]
pub struct WebSocketState {
  sender: Sender,
}

pub fn api() -> Router {
  let socket_channel = SocketChannel::new();
  FileWatcher::new(socket_channel.sender.clone());
  Router::new()
    .route("/", get(ws_handler))
    .with_state(WebSocketState {
      sender: socket_channel.sender,
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
    handle_socket(socket, addr, session.user_id, state.sender)
  })
}

/// WebSocket state machine (one will be spawned per connection)
async fn handle_socket(
  mut socket: WebSocket,
  who: SocketAddr,
  user_id: String,
  socket_sender: Sender,
) {
  if let Err(error) = socket.send(Message::Ping(vec![1, 2, 3])).await {
    log!(err@"Could not send ping to {who}.\n\nError: {error}");
    return;
  }

  log!(success@"Pinged {who}...");

  let (mut sender, mut receiver) = socket.split();

  // Task to notify the client of any updates in the files collection
  let mut socket_receiver = socket_sender.subscribe();
  let event_user_id = user_id.clone();
  let mut send_task = tokio::spawn(async move {
    while let Ok(event) = socket_receiver.recv().await {
      match event {
        SocketEvent::Exit(user_id) if user_id == event_user_id => {
          log!(info@"Files listener task received exit signal, exiting...");
          return;
        }
        SocketEvent::Exit(user_id) => {
          log!(info@"Exit received for {user_id} but id is {event_user_id} so we ignore");
          continue;
        }
        SocketEvent::FileChange(change) => {
          if change.user_id != event_user_id {
            continue;
          }
          let Ok(json) = serde_json::to_string(&change) else {return};
          if let Err(error) = sender.send(Message::Text(json)).await {
            log!(err@"Could not send server message {change:#?}.\n\nError: {error}");
            return;
          }
        }
      }
    }
    log!("Closing connection: {who}...");
    if let Err(error) = sender
      .send(Message::Close(Some(CloseFrame {
        code: close_code::NORMAL,
        reason: Cow::from("Goodbye"),
      })))
      .await
    {
      log!(err@"Could not close connection {who}: {error}");
    }
  });

  // Task to receive messages from the client and log them to the console
  let mut recv_task = tokio::spawn(async move {
    let mut count = 0;
    while let Some(Ok(msg)) = receiver.next().await {
      count += 1;
      if process_message(msg, who).is_break() {
        break;
      }
    }
    count
  });

  // If any one of the tasks exits, send a signal to the other to exit too.
  tokio::select! {
    rv_a = (&mut send_task) => {
      match rv_a {
        Ok(_) => log!(success@"Sent messages to {who}"),
        Err(a) => log!(err@"Error sending messages {a:?}")
      }
    },
    rv_b = (&mut recv_task) => {
      match rv_b {
        Ok(b) => log!(success@"Received {b} messages"),
        Err(b) => log!(err@"Error receiving messages {b:?}")
      }
      if let Err(error) = socket_sender.send(SocketEvent::Exit(user_id)) {
        log!(err@"Error sending exit from message receiver task\n\nError:{error}");
      }
    }
  }

  // Returning from the handler closes the websocket connection
  log!(success@"Websocket context {who} destroyed");
}

fn process_message(msg: Message, who: SocketAddr) -> ControlFlow<(), ()> {
  match msg {
    Message::Text(t) => {
      log!(">>> {who} sent str: {t:?}");
    }
    Message::Binary(d) => {
      log!(">>> {who} sent {} bytes: {d:?}", d.len());
    }
    Message::Close(c) => {
      if let Some(cf) = c {
        log!(
          ">>> {who} sent close with code {} and reason `{}`",
          cf.code,
          cf.reason
        );
      } else {
        log!(">>> {who} somehow sent close message without CloseFrame");
      }
      return ControlFlow::Break(());
    }

    Message::Pong(v) => {
      log!(">>> {who} sent pong with {v:?}");
    }
    // No need to manually handle Message::Ping. But we can access the pings content here.
    Message::Ping(v) => {
      log!(">>> {who} sent ping with {v:?}");
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
