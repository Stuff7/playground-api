mod api;
mod auth;
mod console;
mod db;
mod http;
mod routes;
mod string;
mod tests;
mod websockets;

use auth::{google::GoogleState, session::Session};
use console::Colorize;
use db::{files::system::FileSystem, Database};
use routes::files::FilesRouterState;
use websockets::WebSocketState;

use std::net::SocketAddr;

use format as f;

use axum::{
  extract::FromRef,
  headers::{authorization::Bearer, Authorization},
  http::HeaderValue,
  routing::{delete, get},
  Router, TypedHeader,
};
use reqwest::StatusCode;
use thiserror::Error;
use tokio::signal;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
  let database = Database::new("playground")
    .await
    .unwrap_or_exit("Could not initialize database");
  database.load_sessions().await;
  let state =
    AppState::new(&database).unwrap_or_exit("Could not initialize app state");
  let auth_routes =
    auth::api().unwrap_or_exit("Could not initialize auth routes.");
  let files_api =
    routes::files::api().unwrap_or_exit("Could not initialize files API.");
  let websockets_api = websockets::api();

  let cors = CorsLayer::new()
    .allow_methods(tower_http::cors::Any)
    .allow_headers(tower_http::cors::Any);
  let cors = if let Ok(allowed_origins) = env_var("ALLOWED_ORIGINS") {
    let origins = allowed_origins
      .split(',')
      .map(|origin| origin.parse::<HeaderValue>())
      .collect::<Result<Vec<_>, _>>()
      .unwrap_or_exit(f!(
        "Could not parse allowed origins {allowed_origins:?}"
      ));
    cors.allow_origin(origins)
  } else {
    cors
  };

  let app = Router::new()
    .route("/logout", delete(logout))
    .route("/ping", get(ping))
    .nest("/auth", auth_routes)
    .nest("/api/users", routes::users::api())
    .nest("/api/files", files_api)
    .nest("/ws", websockets_api)
    .with_state(state)
    .layer(cors);

  let socket_address: SocketAddr = env_var("SOCKET_ADDRESS")
    .unwrap_or_exit("Socket address is missing")
    .parse()
    .unwrap_or_exit("Failed to parse socket address");

  log!(success@"listening on {socket_address}");

  axum::Server::bind(&socket_address)
    .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    .with_graceful_shutdown(shutdown_signal(&database))
    .await
    .unwrap_or_exit("Failed to start server");
}

async fn logout(
  TypedHeader(bearer): TypedHeader<Authorization<Bearer>>,
) -> StatusCode {
  Session::invalidate(bearer.token()).await;
  StatusCode::NO_CONTENT
}

async fn ping<'a>() -> &'a str {
  "PONG"
}

pub fn env_var(var_name: &str) -> AppResult<String> {
  std::env::var(var_name).map_err(|_| AppError::Env(var_name.to_string()))
}

async fn shutdown_signal(database: &Database) {
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
  database.save_sessions().await;
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

#[derive(Debug, Clone)]
pub struct AppState {
  database: Database,
  google: GoogleState,
  websockets: WebSocketState,
  files_router: FilesRouterState,
  file_system: FileSystem,
}

impl AppState {
  fn new(database: &Database) -> AppResult<Self> {
    Ok(Self {
      database: database.clone(),
      google: GoogleState::new()?,
      websockets: WebSocketState::new(),
      files_router: FilesRouterState::new(),
      file_system: FileSystem::from(database),
    })
  }
}

impl FromRef<AppState> for Database {
  fn from_ref(state: &AppState) -> Self {
    state.database.clone()
  }
}

impl FromRef<AppState> for GoogleState {
  fn from_ref(state: &AppState) -> Self {
    state.google.clone()
  }
}

impl FromRef<AppState> for WebSocketState {
  fn from_ref(state: &AppState) -> Self {
    state.websockets.clone()
  }
}

impl FromRef<AppState> for FilesRouterState {
  fn from_ref(state: &AppState) -> Self {
    state.files_router.clone()
  }
}

impl FromRef<AppState> for FileSystem {
  fn from_ref(state: &AppState) -> Self {
    state.file_system.clone()
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
