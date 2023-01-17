use std::fmt::Debug;

use crate::GracefulExit;

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation};
use once_cell::sync::Lazy;
use serde::Serialize;

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

pub fn sign_token<T: Serialize + Debug>(data: &T) -> JWTResult<String> {
  encode(&Header::default(), data, &KEYS.encoding).map_err(|err| JWTError::Signing(err))
}

pub fn decode_token<T: serde::de::DeserializeOwned>(token: &str) -> JWTResult<TokenData<T>> {
  decode::<T>(token, &KEYS.decoding, &Validation::default()).map_err(JWTError::from)
}
