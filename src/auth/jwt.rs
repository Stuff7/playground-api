use std::fmt::Debug;

use crate::GracefulExit;

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum JWTError {
  #[error("Error signing JWT: {}\n{:?}", 0.0, 0.1)]
  Signing(jsonwebtoken::errors::Error),
  #[error("Error decoding JWT: {0}")]
  Decoding(#[from] jsonwebtoken::errors::Error),
}

type JWTResult<T = ()> = Result<T, JWTError>;

struct Keys {
  pub encoding: EncodingKey,
  pub decoding: DecodingKey,
}

impl Keys {
  fn new(secret: &[u8]) -> Self {
    Self {
      encoding: EncodingKey::from_secret(secret),
      decoding: DecodingKey::from_secret(secret),
    }
  }
}

static KEYS: Lazy<Keys> = Lazy::new(|| {
  let secret = crate::env_var("JWT_SECRET").unwrap_or_exit("JWT_SECRET must be set");
  Keys::new(secret.as_bytes())
});

pub fn sign_token(sub: &str) -> JWTResult<String> {
  encode(
    &Header::default(),
    &Claims {
      sub: sub.to_string(),
      exp: expires_in(Duration::weeks(2)).timestamp() as usize,
    },
    &KEYS.encoding,
  )
  .map_err(|err| JWTError::Signing(err))
}

pub fn verify_token(token: &str) -> JWTResult<TokenData<Claims>> {
  decode(token, &KEYS.decoding, &Validation::default()).map_err(JWTError::from)
}

fn expires_in(duration: Duration) -> chrono::DateTime<Utc> {
  Utc::now() + duration
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
  pub sub: String,
  exp: usize,
}
