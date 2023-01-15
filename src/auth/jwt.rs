use std::fmt::Debug;

use crate::GracefulExit;

use format as f;

use anyhow::Context;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation};
use once_cell::sync::Lazy;
use serde::Serialize;

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

pub fn sign_token<T: Serialize + Debug>(data: &T) -> anyhow::Result<String> {
  encode(&Header::default(), data, &KEYS.encoding).context(f!("Failed to sign token for {data:?}"))
}

pub fn decode_token<T: serde::de::DeserializeOwned>(token: &str) -> anyhow::Result<TokenData<T>> {
  decode::<T>(token, &KEYS.decoding, &Validation::default())
    .context(f!("Failed to decode token {token:?}"))
}
