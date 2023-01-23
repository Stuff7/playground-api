pub mod google;
pub mod jwt;
pub mod oauth;
pub mod session;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
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
