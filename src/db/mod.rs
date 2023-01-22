mod models;

pub use models::*;

use crate::{
  auth::{
    jwt::{self, JWTError},
    oauth::Token,
  },
  console::Colorize,
  env_var, log, GracefulExit,
};

use mongodb::{
  bson::{doc, Bson, Document},
  options::{
    ClientOptions, FindOneAndUpdateOptions, ReplaceOptions, ResolverConfig, ReturnDocument,
    UpdateOptions,
  },
  Client,
};
use once_cell::sync::{Lazy, OnceCell};
use thiserror::Error;

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
  load_sessions().await;
}

async fn load_sessions() {
  log!(info@"Loading sessions");
  let session = DATABASE
    .0
    .collection::<SessionCache>("sessions")
    .find_one(doc! { "_id": "sessions" }, None)
    .await
    .ok()
    .flatten();
  if let Some(session) = session {
    let sessions = session.sessions;
    SESSIONS_CACHE.lock().await.extend(sessions);
  }
}

pub async fn save_sessions() {
  log!(info@"Saving sessions");
  let upsert = UpdateOptions::builder().upsert(true).build();
  let sessions = SESSIONS_CACHE.lock().await;
  DATABASE
    .0
    .collection::<SessionCache>("sessions")
    .update_one(
      doc! { "_id": "sessions" },
      doc! { "$set": { "sessions": sessions.iter().collect::<Vec<_>>() } },
      upsert,
    )
    .await
    .unwrap_or_exit("Could not save sessions to database");
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
  let mut update = doc! {
    "token.access_token": token.access_token,
    "token.expires_seconds": token.expires_seconds,
  };
  if let Some(refresh_token) = token.refresh_token {
    update.insert("token.refresh_token", refresh_token);
  }
  DATABASE.update_by_id::<Provider>(id, update).await?;
  Ok(())
}

pub async fn find_by_id<T: Collection>(id: &str) -> Option<T> {
  DATABASE
    .find_by_id::<T>(id)
    .await
    .map_err(|err| {
      log!(err@"An error occurred in find_by_id: {err}");
      err
    })
    .ok()
    .flatten()
}

#[derive(Debug)]
pub struct Database(mongodb::Database);

impl Database {
  async fn find_by_id<T: Collection>(&self, id: &str) -> DBResult<Option<T>> {
    let mut cache = T::cache().lock().await;
    if let Some(doc) = cache.get(id).cloned() {
      log!("[find] Cache hit {doc:?}\n");
      return Ok(Some(doc));
    }
    let collection = self.collection::<T>();
    let maybe_doc = collection.find_one(doc! { "_id": id }, None).await?;
    if let Some(doc) = maybe_doc.clone() {
      log!("[find] Caching data {doc:?}\n");
      cache.insert(id.to_string(), doc.clone());
    }
    Ok(maybe_doc)
  }

  async fn update_by_id<T: Collection>(&self, id: &str, update: Document) -> DBResult {
    let collection = self.collection::<T>();
    let options = FindOneAndUpdateOptions::builder()
      .return_document(ReturnDocument::After)
      .build();
    let maybe_doc = collection
      .find_one_and_update(doc! { "_id": id }, doc! { "$set": update }, options)
      .await?;
    if let Some(doc) = maybe_doc {
      log!("[update] Caching data {doc:?}\n");
      T::cache().lock().await.insert(id.to_string(), doc.clone());
    }
    Ok(())
  }

  /// Replace doc in collection or create it if it doesn't exist.
  async fn replace<T: Collection>(&self, doc: &T) -> DBResult {
    let collection = self.collection::<T>();
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
    let collection = self.collection::<T>();
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
