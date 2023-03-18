use std::ops::Deref;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NonEmptyString(String);

impl Default for NonEmptyString {
  fn default() -> Self {
    Self("Empty String".into())
  }
}

impl NonEmptyString {
  fn try_from_str(s: &str) -> StringResult<Self> {
    if s.is_empty() {
      Err(StringError::Empty)
    } else {
      Ok(NonEmptyString(s.to_string()))
    }
  }
}

impl Deref for NonEmptyString {
  type Target = String;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl TryFrom<String> for NonEmptyString {
  type Error = StringError;

  fn try_from(s: String) -> StringResult<Self> {
    NonEmptyString::try_from_str(&s)
  }
}

impl TryFrom<&String> for NonEmptyString {
  type Error = StringError;

  fn try_from(s: &String) -> StringResult<Self> {
    NonEmptyString::try_from_str(s)
  }
}

impl TryFrom<&str> for NonEmptyString {
  type Error = StringError;

  fn try_from(s: &str) -> StringResult<Self> {
    NonEmptyString::try_from_str(s)
  }
}

#[derive(Debug, Error)]
pub enum StringError {
  #[error("String cannot be empty")]
  Empty,
}

pub type StringResult<T = ()> = Result<T, StringError>;
