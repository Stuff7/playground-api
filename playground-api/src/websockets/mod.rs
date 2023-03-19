pub mod channel;
mod event;

use crate::{
  auth::session::SessionQuery, console::Colorize, db::DBError, log,
  websockets::channel::SocketMessage, AppState,
};
use axum::{
  extract::{
    connect_info::ConnectInfo,
    ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade},
    State,
  },
  response::IntoResponse,
  routing::get,
  Router,
};
use channel::{
  EventChannel, EventSender, SocketChannel, SocketReceiver, SocketSender,
};
use event::EventManager;
use futures::{
  sink::SinkExt,
  stream::{SplitSink, SplitStream, StreamExt},
};
use std::{borrow::Cow, net::SocketAddr, ops::ControlFlow};
use thiserror::Error;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct WebSocketState {
  pub event_sender: EventSender,
}

impl WebSocketState {
  pub fn new() -> Self {
    let event_channel = EventChannel::new();
    Self {
      event_sender: event_channel.sender,
    }
  }
}

pub fn api() -> Router<AppState> {
  Router::new().route("/", get(ws_handler))
}

/// The handler for the HTTP request (this gets called when the HTTP GET lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
async fn ws_handler(
  ws: WebSocketUpgrade,
  SessionQuery(session): SessionQuery,
  ConnectInfo(socket_id): ConnectInfo<SocketAddr>,
  State(state): State<WebSocketState>,
) -> impl IntoResponse {
  log!(info@">>> {socket_id} Requested connection");

  ws.on_upgrade(move |socket| {
    handle_socket(
      socket,
      socket_id.to_string(),
      session.user_id,
      state.event_sender,
    )
  })
}

/// WebSocket state machine (one will be spawned per connection)
async fn handle_socket(
  mut socket: WebSocket,
  socket_id: String,
  user_id: String,
  event_sender: EventSender,
) {
  if let Err(error) = socket.send(Message::Ping(vec![1, 2, 3])).await {
    log!(err@">>> {socket_id} Ping send failed: {error}");
    return;
  }

  log!(success@">>> {socket_id} Connected");
  let (raw_socket_sender, raw_socket_receiver) = socket.split();
  let socket_channel = SocketChannel::new();
  let socket_receiver = socket_channel.sender.subscribe();

  let mut send_task = send_client_messages_task(
    socket_receiver,
    socket_id.clone(),
    raw_socket_sender,
  );

  let mut recv_task = receive_client_messages_task(
    raw_socket_receiver,
    socket_channel.sender.clone(),
    event_sender,
    user_id,
    socket_id.clone(),
  );

  // If any one of the tasks exits, send a signal to the other to exit too.
  tokio::select! {
    rv_a = (&mut send_task) => {
      match rv_a {
        Ok(count) => log!(success@">>> {socket_id} Messages sent: {count}"),
        Err(error) => log!(err@">>> {socket_id} Error sending messages: {error:?}")
      }
    },
    rv_b = (&mut recv_task) => {
      match rv_b {
        Ok(count) => log!(success@">>> {socket_id} Messages received: {count}"),
        Err(error) => log!(err@">>> {socket_id} Error receiving messages: {error:?}")
      }
      if let Err(error) = socket_channel.sender.send(SocketMessage::Exit) {
        log!(err@">>> {socket_id} Error sending exit from message receiver task: {error}");
      }
    }
  }

  // Returning from the handler closes the websocket connection
  log!(success@">>> {socket_id} Websocket context destroyed");
}

fn receive_client_messages_task(
  mut raw_socket_receiver: SplitStream<WebSocket>,
  socket_sender: SocketSender,
  event_sender: EventSender,
  user_id: String,
  socket_id: String,
) -> JoinHandle<i32> {
  tokio::spawn(async move {
    let mut event_manager = EventManager::default();
    let mut count = 0;
    while let Some(Ok(msg)) = raw_socket_receiver.next().await {
      count += 1;
      if process_message(&msg, &socket_id).is_break() {
        break;
      }
      if let Message::Text(ref message) = msg {
        event_manager.process_event(
          message,
          &socket_sender,
          &event_sender,
          user_id.clone(),
          socket_id.clone(),
        );
      }
    }
    count
  })
}

fn send_client_messages_task(
  mut socket_receiver: SocketReceiver,
  socket_id: String,
  mut raw_socket_sender: SplitSink<WebSocket, Message>,
) -> JoinHandle<i32> {
  tokio::spawn(async move {
    let mut count = 0;
    while let Ok(event) = socket_receiver.recv().await {
      match event {
        SocketMessage::Exit => {
          log!(info@">>> {socket_id} Main socket task received exit signal, exiting...");
          return count;
        }
        SocketMessage::Message(message) => {
          if let Err(error) = raw_socket_sender.send(message).await {
            log!(err@">>> {socket_id} Could not send server message: {error}");
            break;
          }
          count += 1;
        }
      }
    }
    log!(info@">>> {socket_id} Closing connection...");
    if let Err(error) = raw_socket_sender
      .send(Message::Close(Some(CloseFrame {
        code: close_code::NORMAL,
        reason: Cow::from("Goodbye"),
      })))
      .await
    {
      log!(err@">>> {socket_id} Could not close connection: {error}");
    }
    count
  })
}

fn process_message(msg: &Message, socket_id: &str) -> ControlFlow<(), ()> {
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
