mod api;
mod auth;
mod console;
mod db;
mod http;

use console::Colorize;

use format as f;

use axum::{http::HeaderValue, Router};
use std::net::SocketAddr;
use thiserror::Error;
use tokio::signal;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
  db::init().await;
  let google_api = auth::google::api().unwrap_or_exit("Could not initialize Google API");
  let cors = CorsLayer::new()
    .allow_methods(tower_http::cors::Any)
    .allow_headers(tower_http::cors::Any);
  let cors = if let Ok(allowed_origins) = env_var("ALLOWED_ORIGINS") {
    let origins = allowed_origins
      .split(",")
      .map(|origin| origin.parse::<HeaderValue>())
      .collect::<Result<Vec<_>, _>>()
      .unwrap_or_exit(f!("Could not parse allowed origins {allowed_origins:?}"));
    cors.allow_origin(origins)
  } else {
    cors
  };
  let app = Router::new().nest("/api/google", google_api).layer(cors);

  let socket_address: SocketAddr = env_var("SOCKET_ADDRESS")
    .unwrap_or_exit("Socket address is missing")
    .parse()
    .unwrap_or_exit("Failed to parse socket address");

  log!(success@"listening on {socket_address}");

  axum::Server::bind(&socket_address)
    .serve(app.into_make_service())
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap_or_exit("Failed to start server");
}

pub fn env_var(var_name: &str) -> AppResult<String> {
  std::env::var(var_name).map_err(|_| AppError::Env(var_name.to_string()))
}

async fn shutdown_signal() {
  let ctrl_c = async {
    signal::ctrl_c()
      .await
      .unwrap_or_exit("Failed to install Ctrl+C handler");
  };

  #[cfg(unix)]
  let terminate = async {
    signal::unix::signal(signal::unix::SignalKind::terminate())
      .expect("failed to install signal handler")
      .recv()
      .await;
  };

  #[cfg(not(unix))]
  let terminate = std::future::pending::<()>();

  tokio::select! {
    _ = ctrl_c => {},
    _ = terminate => {},
  }

  log!(info@"Signal received, starting graceful shutdown");
  db::save_sessions().await;
  log!(success@"Graceful shutdown done!");
}

trait GracefulExit<T> {
  fn unwrap_or_exit(self, msg: impl std::fmt::Display) -> T;
}

impl<T, E> GracefulExit<T> for Result<T, E>
where
  E: std::fmt::Display,
{
  fn unwrap_or_exit(self, msg: impl std::fmt::Display) -> T {
    match self {
      Ok(t) => t,
      Err(e) => {
        log!("{msg}: {e}");
        std::process::exit(0)
      }
    }
  }
}

#[derive(Error, Debug)]
pub enum AppError {
  #[error("Missing env var: {}", .0.err())]
  Env(String),
  #[error("{}", .0.to_string().err())]
  UrlParsing(#[from] oauth2::url::ParseError),
}

type AppResult<T = ()> = Result<T, AppError>;
