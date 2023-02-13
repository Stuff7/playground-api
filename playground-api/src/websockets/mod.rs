use crate::{
  console::Colorize,
  db::{DBError, UserFile, DATABASE},
  log,
};

use std::borrow::Cow;
use std::net::SocketAddr;
use std::ops::ControlFlow;

use axum::{
  extract::{
    ws::{close_code, Message, WebSocket, WebSocketUpgrade},
    TypedHeader,
  },
  headers,
  response::IntoResponse,
  routing::get,
  Router,
};

use tokio::sync::oneshot;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::CloseFrame;

use futures::{
  sink::SinkExt,
  stream::{SplitSink, StreamExt},
};

use thiserror::Error;

pub fn api() -> Router {
  Router::new().route("/", get(ws_handler))
}

/// The handler for the HTTP request (this gets called when the HTTP GET lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
async fn ws_handler(
  ws: WebSocketUpgrade,
  user_agent: Option<TypedHeader<headers::UserAgent>>,
  ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
  let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
    user_agent.to_string()
  } else {
    String::from("Unknown browser")
  };
  log!(success@"`{user_agent}` at {addr} connected.");

  ws.on_upgrade(move |socket| handle_socket(socket, addr))
}

/// WebSocket state machine (one will be spawned per connection)
async fn handle_socket(mut socket: WebSocket, who: SocketAddr) {
  if let Err(error) = socket.send(Message::Ping(vec![1, 2, 3])).await {
    log!(err@"Could not send ping to {who}.\n\nError: {error}");
    return;
  }

  log!(success@"Pinged {who}...");

  let (mut sender, mut receiver) = socket.split();

  // Task to notify the client of any updates in the files collection
  let (send_tx, mut send_rx) = oneshot::channel::<()>();
  let mut send_task = tokio::spawn(async move {
    match user_files_listener(&who, &mut send_rx, &mut sender).await {
      Ok(sent_msg_count) => sent_msg_count,
      Err(send_task_err) => {
        log!(err@"There was an error in the send task: {send_task_err}");
        0
      }
    }
  });

  // Task to receive messages from the client and log them to the console
  let (recv_tx, mut recv_rx) = oneshot::channel::<()>();
  let mut recv_task = tokio::spawn(async move {
    let mut count = 0;
    while let Some(Ok(msg)) = receiver.next().await {
      if exit_received(&mut recv_rx) {
        log!(info@"Message processing task received exit signal, exiting...");
        break;
      }
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
        Ok(a) => log!(success@"Sent {a} messages to {who}"),
        Err(a) => log!(err@"Error sending messages {a:?}")
      }
      if recv_tx.send(()).is_err() {
        return;
      }
    },
    rv_b = (&mut recv_task) => {
      match rv_b {
        Ok(b) => log!(success@"Received {b} messages"),
        Err(b) => log!(err@"Error receiving messages {b:?}")
      }
      if send_tx.send(()).is_err() {
        return;
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

async fn user_files_listener(
  connection_id: &SocketAddr,
  send_rx: &mut oneshot::Receiver<()>,
  sender: &mut SplitSink<WebSocket, Message>,
) -> WebSocketResult<u32> {
  log!(info@"Listening for user files changes for connection {connection_id}");
  let mut change_stream = DATABASE.watch::<UserFile>().await?;
  let mut sent_msg_count = 0;

  while let Some(result) = change_stream.next().await {
    if exit_received(send_rx) {
      log!(info@"Files listener task received exit signal, exiting...");
      return Ok(sent_msg_count);
    }
    if let Some(file_change) = result.map_err(DBError::from)?.full_document {
      let message = serde_json::to_string(&file_change)?;

      if let Err(error) = sender.send(Message::Text(message)).await {
        log!(err@"Could not send server message.\n\nError: {error}");
        return Ok(sent_msg_count);
      }
      sent_msg_count += 1;
    }
  }

  log!("Closing connection: {connection_id}...");
  if let Err(error) = sender
    .send(Message::Close(Some(CloseFrame {
      code: close_code::NORMAL,
      reason: Cow::from("Goodbye"),
    })))
    .await
  {
    log!(err@"Could not close connection {connection_id}: {error}");
  }
  Ok(sent_msg_count)
}

fn exit_received<T>(rx: &mut oneshot::Receiver<T>) -> bool {
  let Err(err) = rx.try_recv() else {return true};
  if let oneshot::error::TryRecvError::Closed = err {
    return true;
  }
  false
}

#[derive(Error, Debug)]
pub enum WebSocketError {
  #[error("A JSON error occurred in a WebSocket: {0}")]
  Json(#[from] serde_json::Error),
  #[error("A database error occurred in a WebSocket: {0}")]
  Database(#[from] DBError),
}

type WebSocketResult<T = ()> = Result<T, WebSocketError>;
