use std::collections::{HashMap, HashSet};

use mongodb::bson::doc;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::auth::oauth::Token;

pub type Cache<T> = Lazy<Mutex<HashMap<String, T>>>;

pub static SESSIONS_CACHE: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
pub static USERS_CACHE: Cache<User> = Lazy::new(|| Mutex::new(HashMap::new()));
pub static PROVIDERS_CACHE: Cache<Provider> = Lazy::new(|| Mutex::new(HashMap::new()));

pub trait Collection:
  std::fmt::Debug + Serialize + DeserializeOwned + Unpin + Send + Sync + Clone + 'static
{
  fn collection_name() -> &'static str;
  fn id(&self) -> &str;
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized;
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
  pub picture: String,
  pub linked_accounts: HashSet<String>,
}

impl User {
  pub fn new(provider_id: String, picture: String) -> Self {
    Self {
      _id: provider_id.clone(),
      picture,
      linked_accounts: HashSet::from([provider_id]),
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
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized,
  {
    &USERS_CACHE
  }
}

impl From<Provider> for User {
  fn from(provider: Provider) -> Self {
    Self::new(provider._id, provider.picture)
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
  #[serde(rename = "_id")]
  pub _id: String,
  pub picture: String,
  pub token: Token,
}

impl Provider {
  pub fn new(
    _id: String,
    picture: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_seconds: u32,
  ) -> Self {
    Self {
      _id,
      picture,
      token: Token {
        access_token,
        refresh_token,
        expires_seconds,
      },
    }
  }
}

impl Collection for Provider {
  fn collection_name() -> &'static str {
    "providers"
  }
  fn id(&self) -> &str {
    &self._id
  }
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized,
  {
    &PROVIDERS_CACHE
  }
}
