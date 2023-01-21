pub mod google;
pub mod jwt;
pub mod oauth;
pub mod session;

use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::sync::Mutex;

pub type Cache<T> = Lazy<Mutex<HashMap<String, T>>>;

#[derive(Debug, Deserialize)]
struct AuthenticateQuery {
  current_login: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizedQuery {
  code: String,
  state: String,
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
  token: String,
  user: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserIDQuery {
  user: Option<String>,
}
