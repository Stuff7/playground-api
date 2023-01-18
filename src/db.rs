use crate::{
  auth::{
    jwt::{self, JWTError},
    oauth::Token,
  },
  console::Colorize,
  env_var, log, GracefulExit,
};

use mongodb::{
  bson::doc,
  options::{ClientOptions, ReplaceOptions, ResolverConfig},
  Client,
};
use once_cell::sync::{Lazy, OnceCell};
use std::collections::HashSet;
use thiserror::Error;

use serde::{Deserialize, Serialize};

// First we load the database within the main async runtime
static DATABASE_RESULT: OnceCell<Database> = OnceCell::new();
// Then we get the database lazily, exiting the app if the database was not initialized
static DATABASE: Lazy<&Database> = Lazy::new(|| {
  DATABASE_RESULT
    .get()
    .ok_or_else(|| DBError::Uninitialized)
    .unwrap_or_exit("Tried to access database before initialization")
});

pub async fn init() {
  let client_options = ClientOptions::parse_with_resolver_config(
    env_var("MONGODB_URI").unwrap_or_exit("Could not find MongoDB URI"),
    ResolverConfig::cloudflare(),
  )
  .await
  .unwrap_or_exit("Could not parse MongoDB URI");

  let client =
    Client::with_options(client_options).unwrap_or_exit("Could not initialize MongoDB client");

  DATABASE_RESULT
    .set(Database(client.database("playground")))
    .map_err(DBError::AlreadyInitialized)
    .unwrap_or_exit("Database was initialized more than once");
  log!(info@"Database Initialized");
}

pub async fn save_user(provider: Provider) -> DBResult<String> {
  let user: User = provider.clone().into();
  let token = jwt::sign_token(&user)?;
  DATABASE
    .insert_doc(&Collection::Users, &user._id, &user)
    .await?;
  DATABASE
    .insert_doc(&Collection::Providers, &provider._id, &provider)
    .await?;
  Ok(token)
}

pub async fn add_provider_to_user(mut user: User, provider: Provider) -> DBResult<String> {
  DATABASE
    .insert_doc(&Collection::Providers, &provider._id, &provider)
    .await?;
  user.linked_accounts.insert(provider._id);
  let token = jwt::sign_token(&user)?;
  DATABASE
    .insert_doc(&Collection::Users, &user._id, &user)
    .await?;
  Ok(token)
}

pub async fn update_provider_token(id: &str, token: Token) -> DBResult {
  let providers = DATABASE.collection(&Collection::Providers);
  let mut update = doc! {
    "token.access_token": token.access_token,
    "token.expires_seconds": token.expires_seconds as f32,
  };
  if let Some(refresh_token) = token.refresh_token {
    update.insert("refresh_token", refresh_token);
  }
  providers
    .find_one_and_update(doc! { "_id": id }, doc! { "$set": update }, None)
    .await?;
  Ok(())
}

pub async fn get_provider_by_id(id: &str) -> Option<Provider> {
  DATABASE
    .collection(&Collection::Providers)
    .find_one(doc! { "_id": id }, None)
    .await
    .ok()
    .flatten()
}

#[derive(Debug)]
enum Collection {
  Providers,
  Users,
}

impl std::fmt::Display for Collection {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", format!("{:?}", self).to_lowercase())
  }
}

#[derive(Debug)]
pub struct Database(mongodb::Database);

impl Database {
  /// Update doc in collection or create it if it doesn't exist.
  async fn insert_doc<T: Serialize>(&self, collection: &Collection, id: &str, doc: &T) -> DBResult {
    let collection = self.0.collection::<T>(&collection.to_string());
    let upsert = ReplaceOptions::builder().upsert(true).build();
    collection
      .replace_one(doc! { "_id": id }, doc, upsert)
      .await?;
    Ok(())
  }

  fn collection(&self, collection: &Collection) -> mongodb::Collection<Provider> {
    self.0.collection(&collection.to_string())
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
  pub _id: String,
  pub picture: String,
  pub linked_accounts: HashSet<String>,
  pub exp: usize,
}

impl User {
  pub fn new(provider_id: String, picture: String) -> Self {
    Self {
      _id: provider_id.clone(),
      picture,
      linked_accounts: HashSet::from([provider_id]),
      exp: 2000000000, // May 2033
    }
  }
}

impl From<Provider> for User {
  fn from(provider: Provider) -> Self {
    Self::new(provider._id, provider.picture)
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Provider {
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
    expires_seconds: u64,
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

#[derive(Error, Debug)]
pub enum DBError {
  #[error(transparent)]
  InternalDatabase(#[from] mongodb::error::Error),
  #[error("Database has not been initialized")]
  Uninitialized,
  #[error("Database has already been initialized as {0:?}")]
  AlreadyInitialized(Database),
  #[error(transparent)]
  JWT(#[from] JWTError),
}

type DBResult<T = ()> = Result<T, DBError>;
