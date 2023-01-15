pub mod google;
pub mod jwt;
pub mod oauth;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AuthRequest {
  code: String,
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
  token: String,
  user: String,
}

#[derive(Debug, Deserialize)]
struct UserIDQuery {
  user: String,
}
