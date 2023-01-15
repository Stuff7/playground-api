use oauth2::{
  basic::{BasicClient, BasicTokenType},
  reqwest::async_http_client,
  AuthorizationCode, EmptyExtraTokenFields, RefreshToken, StandardTokenResponse, TokenResponse,
};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Token {
  pub access_token: String,
  pub refresh_token: Option<String>,
  pub expires_seconds: u64,
}

impl From<StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>> for Token {
  fn from(token: StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>) -> Self {
    Self {
      expires_seconds: token.expires_in().unwrap_or_default().as_secs(),
      access_token: token.access_token().secret().clone(),
      refresh_token: token
        .refresh_token()
        .and_then(|refresh| Some(refresh.secret().clone())),
    }
  }
}

impl Token {
  pub async fn exchange(client: &BasicClient, code: String) -> anyhow::Result<Self> {
    let token = client
      .exchange_code(AuthorizationCode::new(code))
      .request_async(async_http_client)
      .await?;
    Ok(token.into())
  }

  pub async fn refresh(&mut self, client: &BasicClient) -> anyhow::Result<&Self> {
    if let Some(refresh_token) = &self.refresh_token {
      let token = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
        .request_async(async_http_client)
        .await?;
      *self = token.into();
      return Ok(self);
    }
    Err(TokenError::NoRefreshToken.into())
  }

  pub async fn request(
    &mut self,
    oauth_client: &BasicClient,
    request: reqwest::RequestBuilder,
  ) -> Result<Response, TokenError> {
    match self
      .try_request(
        request
          .try_clone()
          .ok_or_else(|| TokenError::InvalidRequestBody)?,
      )
      .await
    {
      Ok(response) => Ok(response),
      Err(_) => {
        self.refresh(oauth_client).await?;
        self.try_request(request).await
      }
    }
  }

  async fn try_request(&self, request: reqwest::RequestBuilder) -> Result<Response, TokenError> {
    request
      .bearer_auth(self.access_token.clone())
      .send()
      .await?
      .error_for_status()
      .map_err(TokenError::from)
  }
}

#[derive(Error, Debug)]
pub enum TokenError {
  #[error("Oauth request returned bad status")]
  BadStatus(#[from] reqwest::Error),
  #[error("Oauth request body cannot be cloned")]
  InvalidRequestBody,
  #[error("Missing refresh token")]
  NoRefreshToken,
  #[error("Invalid refresh token")]
  InvalidRefreshToken(#[from] anyhow::Error),
}
