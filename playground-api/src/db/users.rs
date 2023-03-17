use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::auth::jwt;

use super::{files::File, Collection, DBResult, Database};

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

pub async fn save_user(user: &User, database: &Database) -> DBResult<String> {
  let token = jwt::sign_token(&user._id)?;
  if let Some(user) = database.create(user, None).await? {
    database
      .create(&File::new_root_folder(user._id.clone())?, None)
      .await?;
  }
  Ok(token)
}
