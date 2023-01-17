use crate::{
  auth::{
    jwt::{self, JWTError},
    oauth::Token,
  },
  GracefulExit,
};

use once_cell::sync::Lazy;
use std::{
  collections::{HashMap, HashSet},
  fs,
};
use thiserror::Error;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const DATABASE_PATH: &str = "db.json";

static DATABASE: Lazy<Mutex<Database>> = Lazy::new(|| Mutex::new(Database::load()));

pub async fn save_user(provider: Provider) -> DBResult<String> {
  let user: User = provider.clone().into();
  let token = jwt::sign_token(&user)?;
  let mut database = DATABASE.lock().await;
  database.users.insert(user._id.clone(), user);
  database.providers.insert(provider._id.clone(), provider);
  database.save()?;
  Ok(token)
}

pub async fn add_provider_to_user(mut user: User, provider: Provider) -> DBResult<String> {
  let mut database = DATABASE.lock().await;
  let provider_id = provider._id.clone();
  database.providers.insert(provider_id.clone(), provider);
  user.linked_accounts.insert(provider_id);
  let token = jwt::sign_token(&user)?;
  database.users.insert(user._id.clone(), user);
  database.save()?;
  Ok(token)
}

pub async fn update_provider_token(id: &str, mut token: Token) -> DBResult {
  let mut database = DATABASE.lock().await;
  if let Some(provider) = database.providers.get_mut(id) {
    token.refresh_token = token.refresh_token.or(provider.token.refresh_token.clone());
    provider.token = token;
    database.save()?;
  }
  Ok(())
}

pub async fn get_provider_by_id(id: &str) -> Option<Provider> {
  let database = DATABASE.lock().await;
  database.providers.get(id).cloned()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Database {
  users: HashMap<String, User>,
  providers: HashMap<String, Provider>,
}

impl Database {
  fn load() -> Self {
    match fs::read(DATABASE_PATH) {
      Ok(data) => {
        Ok(serde_json::from_slice(&data[..]).unwrap_or_exit("Failed to parse database file"))
      }
      Err(err) => match err.kind() {
        std::io::ErrorKind::NotFound => {
          fs::File::create(DATABASE_PATH).unwrap_or_exit("Failed to create database file");
          let database: Self = serde_json::from_str(r#"{"users":{},"providers":{}}"#)
            .unwrap_or_exit("Failed to initialize database data");
          database
            .save()
            .unwrap_or_exit("Failed to create database file");
          Ok(database)
        }
        _ => Err(err),
      },
    }
    .unwrap_or_exit("Failed to read database file")
  }

  fn save(&self) -> DBResult {
    fs::write(
      DATABASE_PATH,
      serde_json::to_string(self).map_err(DBError::from)?,
    )
    .map_err(DBError::from)?;
    Ok(())
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
  JWT(#[from] JWTError),
  #[error("Database I/O error")]
  IO(#[from] std::io::Error),
  #[error("Database parsing error")]
  Parsing(#[from] serde_json::Error),
}

type DBResult<T = ()> = Result<T, DBError>;
