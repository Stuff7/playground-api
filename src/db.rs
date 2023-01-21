use crate::{
  auth::{
    jwt::{self, JWTError},
    oauth::Token,
    Cache,
  },
  console::Colorize,
  env_var, log, GracefulExit,
};

use mongodb::{
  bson::{doc, Bson},
  options::{ClientOptions, ReplaceOptions, ResolverConfig, UpdateOptions},
  Client,
};
use once_cell::sync::{Lazy, OnceCell};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tokio::sync::Mutex;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

// First we load the database within the main async runtime
static DATABASE_RESULT: OnceCell<Database> = OnceCell::new();
// Then we get the database lazily, exiting the app if the database was not initialized
static DATABASE: Lazy<&Database> = Lazy::new(|| {
  DATABASE_RESULT
    .get()
    .ok_or_else(|| DBError::Uninitialized)
    .unwrap_or_exit("Tried to access database before initialization")
});

pub static USERS_CACHE: Cache<User> = Lazy::new(|| Mutex::new(HashMap::new()));
pub static PROVIDERS_CACHE: Cache<Provider> = Lazy::new(|| Mutex::new(HashMap::new()));

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
  let token = jwt::sign_token(&user._id)?;
  DATABASE.create(&user).await?;
  DATABASE.replace(&provider).await?;
  Ok(token)
}

pub async fn add_provider_to_user(mut user: User, provider: Provider) -> DBResult<String> {
  DATABASE.replace(&provider).await?;
  user.linked_accounts.insert(provider._id);
  let token = jwt::sign_token(&user._id)?;
  DATABASE.replace(&user).await?;
  Ok(token)
}

pub async fn update_provider_token(id: &str, token: Token) -> DBResult {
  let providers = DATABASE.collection::<Provider>();
  let mut update = doc! {
    "token.access_token": token.access_token,
    "token.expires_seconds": token.expires_seconds,
  };
  if let Some(refresh_token) = token.refresh_token {
    update.insert("token.refresh_token", refresh_token);
  }
  providers
    .find_one_and_update(doc! { "_id": id }, doc! { "$set": update }, None)
    .await?;
  Ok(())
}

pub async fn get_by_id<T: Collection>(id: &str) -> Option<T> {
  let cache = T::cache().lock().await.get(id).cloned();
  if cache.is_some() {
    log!("Cache hit {cache:?}\n");
    return cache;
  }
  DATABASE
    .collection::<T>()
    .find_one(doc! { "_id": id }, None)
    .await
    .map_err(|err| {
      log!(err@"An error occurred in get_by_id: {err}");
      err
    })
    .ok()
    .flatten()
}

pub trait Collection:
  std::fmt::Debug + Serialize + DeserializeOwned + Unpin + Send + Sync + Clone + 'static
{
  fn collection_name() -> &'static str;
  fn id(&self) -> &str;
  fn cache() -> &'static Cache<Self>
  where
    Self: Sized;
}

#[derive(Debug)]
pub struct Database(mongodb::Database);

impl Database {
  /// Replace doc in collection or create it if it doesn't exist.
  async fn replace<T: Collection>(&self, doc: &T) -> DBResult {
    let collection = self.0.collection::<T>(T::collection_name());
    let upsert = ReplaceOptions::builder().upsert(true).build();
    collection
      .replace_one(doc! { "_id": doc.id() }, doc, upsert)
      .await?;
    log!("[replace] Caching data {doc:?}\n");
    T::cache()
      .lock()
      .await
      .insert(doc.id().to_string(), doc.clone());
    Ok(())
  }

  /// Insert doc only if it doesn't exist.
  async fn create<T: Collection + Into<Bson>>(&self, doc: &T) -> DBResult {
    let collection = self.0.collection::<T>(T::collection_name());
    let upsert = UpdateOptions::builder().upsert(true).build();
    let result = collection
      .update_one(
        doc! { "_id": doc.id() },
        doc! { "$setOnInsert": doc },
        upsert,
      )
      .await?;
    let doc_to_save = if result.matched_count == 1 && result.modified_count == 0 {
      collection
        .find_one(doc! { "_id": doc.id() }, None)
        .await?
        .ok_or_else(|| {
          DBError::Logic(
            "The new doc was not created even though it didn't exist (This should never happen)"
              .to_string(),
          )
        })?
    } else {
      doc.clone()
    };
    log!("[create] Caching data {doc_to_save:?}\n");
    T::cache()
      .lock()
      .await
      .insert(doc_to_save.id().to_string(), doc_to_save);
    Ok(())
  }

  fn collection<T: Collection>(&self) -> mongodb::Collection<T> {
    self.0.collection(T::collection_name())
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
  pub _id: String,
  pub picture: String,
  pub linked_accounts: HashSet<String>,
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

impl From<User> for Bson {
  fn from(user: User) -> Self {
    Bson::Document(doc! {
      "_id": user._id,
      "picture": user.picture,
      "linked_accounts": Vec::from_iter(user.linked_accounts),
    })
  }
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
  #[error("Logical Error: {0}")]
  Logic(String),
}

type DBResult<T = ()> = Result<T, DBError>;
