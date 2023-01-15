use crate::{
  auth::{jwt, oauth::Token},
  GracefulExit,
};

use std::{
  collections::{HashMap, HashSet},
  fs,
};
use thiserror::Error;

use serde::{Deserialize, Serialize};

const DATABASE_PATH: &str = "db.json";

pub async fn save_user(provider: Provider) -> anyhow::Result<String> {
  let user: User = provider.clone().into();
  let token = jwt::sign_token(&user)?;
  let mut database = get_database();
  database.users.insert(user._id.clone(), user);
  database.providers.insert(provider._id.clone(), provider);
  update_database(&database)?;
  Ok(token)
}

pub async fn update_provider_token(id: &str, mut token: Token) -> Result<(), DBError> {
  let mut database = get_database();
  if let Some(provider) = database.providers.get_mut(id) {
    token.refresh_token = token.refresh_token.or(provider.token.refresh_token.clone());
    provider.token = token;
    update_database(&database)?;
  }
  Ok(())
}

pub fn get_provider_by_id(id: &str) -> Option<Provider> {
  let database = get_database();
  database.providers.get(id).cloned()
}

fn get_database() -> Database {
  match fs::read(DATABASE_PATH) {
    Ok(data) => {
      Ok(serde_json::from_slice(&data[..]).unwrap_or_exit("Failed to parse database file"))
    }
    Err(err) => match err.kind() {
      std::io::ErrorKind::NotFound => {
        fs::File::create(DATABASE_PATH).unwrap_or_exit("Failed to create database file");
        let database: Database = serde_json::from_str(r#"{"users":{},"providers":{}}"#)
          .unwrap_or_exit("Failed to initialize database data");
        update_database(&database).unwrap_or_exit("Failed to create database file");
        Ok(database)
      }
      _ => Err(err),
    },
  }
  .unwrap_or_exit("Failed to read database file")
}

fn update_database<T: Serialize>(database: &T) -> Result<(), DBError> {
  fs::write(
    DATABASE_PATH,
    serde_json::to_string(database).map_err(DBError::from)?,
  )
  .map_err(DBError::from)?;
  Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Database {
  users: HashMap<String, User>,
  providers: HashMap<String, Provider>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
  pub _id: String,
  pub linked_accounts: HashSet<String>,
  pub exp: usize,
}

impl User {
  pub fn new(provider_id: String) -> Self {
    Self {
      _id: provider_id.clone(),
      linked_accounts: HashSet::from([provider_id]),
      exp: 2000000000, // May 2033
    }
  }
}

impl From<Provider> for User {
  fn from(provider: Provider) -> Self {
    Self::new(provider._id)
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Provider {
  pub _id: String,
  pub token: Token,
}

impl Provider {
  pub fn new(
    _id: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_seconds: u64,
  ) -> Self {
    Self {
      _id,
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
  #[error("Database I/O error")]
  IO(#[from] std::io::Error),
  #[error("Database parsing error")]
  Parsing(#[from] serde_json::Error),
}
