pub mod files;
pub mod users;

use std::{collections::HashMap, time::Duration};

use serde::{de::DeserializeOwned, Serialize};

use crate::{
  auth::{
    jwt::JWTError,
    session::{SessionCache, SESSIONS_CACHE},
  },
  console::Colorize,
  env_var, log,
  string::StringError,
  AppError, GracefulExit,
};

use mongodb::{
  bson::{self, to_document, Bson, Document},
  options::{
    Acknowledgment, ClientOptions, FindOneAndUpdateOptions, InsertManyOptions,
    ReplaceOptions, ResolverConfig, UpdateOptions, WriteConcern,
  },
  results::UpdateResult,
  Client, Cursor,
};
use thiserror::Error;

pub use mongodb::bson::doc;
pub use mongodb::options::ReturnDocument;

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

#[derive(Debug, Clone)]
pub struct Database(mongodb::Database);

impl Database {
  pub async fn new(db_name: &str) -> DBResult<Self> {
    let client_options = ClientOptions::parse_with_resolver_config(
      env_var("MONGODB_URI")?,
      ResolverConfig::cloudflare(),
    )
    .await?;

    let client = Client::with_options(client_options)?;

    let db = Self(client.database(db_name));
    log!(info@"Database {db_name:?} initialized");
    Ok(db)
  }

  pub async fn save_sessions(&self) {
    log!(info@"Saving sessions");
    let upsert = UpdateOptions::builder().upsert(true).build();
    let sessions = SESSIONS_CACHE.lock().await;
    self
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

  pub async fn load_sessions(&self) {
    log!(info@"Loading sessions");
    let session = self
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

  pub async fn find_many<T: Collection>(
    &self,
    query: Document,
  ) -> DBResult<Vec<T>> {
    let collection = self.collection::<T>();
    let mut cursor = collection.find(query, None).await?;
    let mut documents = Vec::new();
    while cursor.advance().await? {
      let document = cursor.deserialize_current()?;
      documents.push(document);
    }
    Ok(documents)
  }

  pub async fn find_by_id<T: Collection>(
    &self,
    id: &str,
  ) -> DBResult<Option<T>> {
    let collection = self.collection::<T>();
    Ok(collection.find_one(doc! { "_id": id }, None).await?)
  }

  pub async fn aggregate<T: Collection>(
    &self,
    pipeline: impl IntoIterator<Item = Document>,
  ) -> DBResult<Cursor<T>> {
    let result = self
      .collection::<T>()
      .aggregate(pipeline, None)
      .await?
      .with_type::<T>();
    Ok(result)
  }

  #[allow(dead_code)]
  pub async fn delete<T: Collection>(
    &self,
    query: Document,
  ) -> DBResult<Option<T>> {
    let collection = self.collection::<T>();
    Ok(collection.find_one_and_delete(query, None).await?)
  }

  pub async fn delete_many<T: Collection>(
    &self,
    query: Document,
  ) -> DBResult<u64> {
    let collection = self.collection::<T>();
    Ok(collection.delete_many(query, None).await?.deleted_count)
  }

  pub async fn update<T: Collection>(
    &self,
    update: Document,
    query: Document,
    return_document: Option<ReturnDocument>,
  ) -> DBResult<Option<T>> {
    let collection = self.collection::<T>();
    let options = FindOneAndUpdateOptions::builder()
      .return_document(return_document.unwrap_or(ReturnDocument::After))
      .build();
    Ok(
      collection
        .find_one_and_update(query, doc! { "$set": update }, options)
        .await?,
    )
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

  #[allow(dead_code)]
  /// Replace doc in collection or create it if it doesn't exist.
  pub async fn replace<T: Collection>(
    &self,
    doc: &T,
    query: Option<Document>,
  ) -> DBResult {
    let collection = self.collection::<T>();
    let upsert = ReplaceOptions::builder().upsert(true).build();
    collection
      .replace_one(
        query.unwrap_or_else(|| doc! { "_id": doc.id() }),
        doc,
        upsert,
      )
      .await?;
    Ok(())
  }

  /// Insert doc only if it doesn't exist.
  pub async fn create<'a, T: Collection>(
    &self,
    doc: &'a T,
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
    Ok(result.upserted_id.is_some().then_some(doc.clone()))
  }

  #[allow(dead_code)]
  /// Insert docs only if they don't exist.
  pub async fn create_many<'a, T: Collection>(
    &self,
    docs: &[T],
  ) -> DBResult<HashMap<usize, Bson>> {
    let collection = self.collection::<T>();
    let options = InsertManyOptions::builder()
      .ordered(false)
      .write_concern(
        WriteConcern::builder()
          .w(Acknowledgment::Majority)
          .w_timeout(Duration::from_secs(5))
          .build(),
      )
      .build();
    let result = collection.insert_many(docs, options).await?;
    Ok(result.inserted_ids)
  }

  pub fn collection<T: Collection>(&self) -> mongodb::Collection<T> {
    self.0.collection(T::collection_name())
  }
}

#[derive(Error, Debug)]
pub enum DBError {
  #[error(transparent)]
  Application(#[from] AppError),
  #[error(transparent)]
  InternalDatabase(#[from] mongodb::error::Error),
  #[error(transparent)]
  Jwt(#[from] JWTError),
  #[error("Error serializing bson: {0}")]
  Bson(#[from] bson::ser::Error),
  #[error("Error parsing object id: {0}")]
  BsonOid(#[from] bson::oid::Error),
  #[error("String Error: {0}")]
  String(#[from] StringError),
}

type DBResult<T = ()> = Result<T, DBError>;
