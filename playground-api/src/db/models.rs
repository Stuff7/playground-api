use std::collections::HashSet;

use mongodb::bson::doc;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::Mutex;

pub static SESSIONS_CACHE: Lazy<Mutex<HashSet<String>>> =
  Lazy::new(|| Mutex::new(HashSet::new()));

pub trait Collection:
  std::fmt::Debug
  + Serialize
  + DeserializeOwned
  + Unpin
  + Send
  + Sync
  + Clone
  + 'static
{
  fn collection_name() -> &'static str;
  fn id(&self) -> &str;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionCache {
  _id: String,
  pub sessions: HashSet<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct User {
  #[serde(rename = "_id")]
  pub _id: String,
  pub name: String,
  pub picture: String,
}

impl User {
  pub fn new(id: &str, name: &str, picture: &str) -> Self {
    Self {
      _id: id.to_string(),
      name: name.to_string(),
      picture: picture.to_string(),
    }
  }
}

impl Collection for User {
  fn collection_name() -> &'static str {
    "users"
  }
  fn id(&self) -> &str {
    &self._id
  }
}
