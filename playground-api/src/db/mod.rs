mod models;

pub use models::*;

use crate::{
  auth::jwt::{self, JWTError},
  console::Colorize,
  env_var, log, GracefulExit,
};

use mongodb::{
  bson::{self, doc, to_document, Document},
  options::{
    ClientOptions, FindOneAndUpdateOptions, ReplaceOptions, ResolverConfig, ReturnDocument,
    UpdateOptions,
  },
  results::UpdateResult,
  Client,
};
use once_cell::sync::{Lazy, OnceCell};
use thiserror::Error;

use format as f;

// First we load the database within the main async runtime
static DATABASE_RESULT: OnceCell<Database> = OnceCell::new();
// Then we get the database lazily, exiting the app if the database was not initialized
pub static DATABASE: Lazy<&Database> = Lazy::new(|| {
  DATABASE_RESULT
    .get()
    .ok_or(DBError::Uninitialized)
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

pub async fn save_user(user: &User) -> DBResult<String> {
  let token = jwt::sign_token(&user._id)?;
  if let Some(user) = DATABASE.create(user, None).await? {
    DATABASE
      .create(&UserFile::new_root_folder(user._id), None)
      .await?;
  }
  Ok(token)
}

pub async fn save_file(file: &UserFile) -> DBResult<Option<UserFile>> {
  let mut query = &mut PartialUserFile::default();
  query.user_id = Some(file.user_id.clone());
  query.folder_id = Some(file.folder_id.clone());
  query.name = Some(file.name.clone());
  DATABASE.create(file, Some(to_document(query)?)).await
}

#[derive(Debug)]
pub struct Database(mongodb::Database);

impl Database {
  pub async fn find_many<T: Collection>(&self, query: Document) -> DBResult<Vec<T>> {
    let collection = self.collection::<T>();
    let mut cursor = collection.find(query, None).await?;
    let mut documents = Vec::new();
    while cursor.advance().await? {
      let document = cursor.deserialize_current()?;
      documents.push(document);
    }
    Ok(documents)
  }

  pub async fn find_by_id<T: Collection>(&self, id: &str) -> DBResult<Option<T>> {
    let mut cache = T::cache().lock().await;
    if let Some(doc) = cache.get(id).cloned() {
      log!("[find] Cache hit {doc:?}\n");
      return Ok(Some(doc));
    }
    let collection = self.collection::<T>();
    let maybe_doc = collection.find_one(doc! { "_id": id }, None).await?;
    if let Some(doc) = maybe_doc.clone() {
      log!("[find] Caching data {doc:?}\n");
      cache.insert(id.to_string(), doc);
    }
    Ok(maybe_doc)
  }

  pub async fn delete<T: Collection>(&self, query: Document) -> DBResult<Option<T>> {
    let collection = self.collection::<T>();
    let maybe_doc = collection.find_one_and_delete(query, None).await?;
    let mut cache = T::cache().lock().await;
    if let Some(ref doc) = maybe_doc {
      log!("[delete] Clearing cache {doc:?}\n");
      cache.remove(doc.id());
    }
    Ok(maybe_doc)
  }

  pub async fn update<T: Collection>(
    &self,
    update: Document,
    query: Document,
  ) -> DBResult<Option<T>> {
    let collection = self.collection::<T>();
    let options = FindOneAndUpdateOptions::builder()
      .return_document(ReturnDocument::After)
      .build();
    let maybe_doc = collection
      .find_one_and_update(query, doc! { "$set": update }, options)
      .await?;
    if let Some(ref doc) = maybe_doc {
      log!("[update] Caching data {doc:?}\n");
      T::cache()
        .lock()
        .await
        .insert(doc.id().to_string(), doc.clone());
    }
    Ok(maybe_doc)
  }

  pub async fn update_many<T: Collection>(
    &self,
    update: Document,
    query: Document,
  ) -> DBResult<UpdateResult> {
    let collection = self.collection::<T>();
    let result = collection
      .update_many(query, doc! { "$set": update }, None)
      .await?;
    Ok(result)
  }

  /// Replace doc in collection or create it if it doesn't exist.
  #[allow(dead_code)]
  pub async fn replace<T: Collection>(&self, doc: &T, query: Option<Document>) -> DBResult {
    let collection = self.collection::<T>();
    let upsert = ReplaceOptions::builder().upsert(true).build();
    collection
      .replace_one(
        query.unwrap_or_else(|| doc! { "_id": doc.id() }),
        doc,
        upsert,
      )
      .await?;
    log!("[replace] Caching data {doc:?}\n");
    T::cache()
      .lock()
      .await
      .insert(doc.id().to_string(), doc.clone());
    Ok(())
  }

  /// Insert doc only if it doesn't exist.
  pub async fn create<T: Collection>(
    &self,
    doc: &T,
    query: Option<Document>,
  ) -> DBResult<Option<T>> {
    let collection = self.collection::<T>();
    let upsert = UpdateOptions::builder().upsert(true).build();
    let result = collection
      .update_one(
        query.unwrap_or_else(|| doc! { "_id": doc.id() }),
        doc! { "$setOnInsert": to_document(&doc)? },
        upsert,
      )
      .await?;
    let mut updated_doc = None;
    let update_done = result.upserted_id.is_some();
    if update_done {
      let id = doc.id();
      log!("[create] Caching data {doc:?}\n");
      let cached_doc = self.find_by_id::<T>(id).await?.ok_or_else(|| {
        DBError::Fatal(f!("Doc with id {id:?} was updated but it was not found",))
      })?;
      T::cache()
        .lock()
        .await
        .insert(id.to_string(), cached_doc.clone());
      updated_doc = Some(cached_doc);
    }
    Ok(updated_doc)
  }

  pub fn collection<T: Collection>(&self) -> mongodb::Collection<T> {
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
  Jwt(#[from] JWTError),
  #[error("Error serializing bson: {0}")]
  Bson(#[from] bson::ser::Error),
  #[error("Error parsing object id: {0}")]
  BsonOid(#[from] bson::oid::Error),
  #[error("Fatal database error: {0}")]
  Fatal(String),
}

type DBResult<T = ()> = Result<T, DBError>;