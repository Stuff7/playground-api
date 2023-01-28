use oauth2::{
  basic::{BasicClient, BasicErrorResponseType, BasicTokenType},
  reqwest::async_http_client,
  AuthorizationCode, EmptyExtraTokenFields, RefreshToken, RequestTokenError, StandardErrorResponse,
  StandardTokenResponse, TokenResponse,
};
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

type AsyncRequestError = RequestTokenError<
  oauth2::reqwest::Error<reqwest::Error>,
  StandardErrorResponse<BasicErrorResponseType>,
>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Token {
  pub access_token: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub refresh_token: Option<String>,
  pub expires_seconds: u32,
}

impl From<StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>> for Token {
  fn from(token: StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>) -> Self {
    Self {
      expires_seconds: token.expires_in().unwrap_or_default().as_secs() as u32,
      access_token: token.access_token().secret().clone(),
      refresh_token: token
        .refresh_token()
        .map(|refresh| refresh.secret().clone()),
    }
  }
}

impl Token {
  pub async fn exchange(client: &BasicClient, code: String) -> OAuthResult<Self> {
    let token = client
      .exchange_code(AuthorizationCode::new(code))
      .request_async(async_http_client)
      .await?;
    Ok(token.into())
  }

  pub async fn refresh(&mut self, client: &BasicClient) -> OAuthResult<&Self> {
    if let Some(refresh_token) = &self.refresh_token {
      let token = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
        .request_async(async_http_client)
        .await?;
      *self = token.into();
      return Ok(self);
    }
    Err(OAuthError::NoRefreshToken)
  }

  pub async fn request(
    &mut self,
    oauth_client: &BasicClient,
    request: reqwest::RequestBuilder,
  ) -> OAuthResult<Response> {
    match self
      .try_request(request.try_clone().ok_or(OAuthError::InvalidRequestBody)?)
      .await
    {
      Ok(response) => Ok(response),
      Err(_) => {
        self.refresh(oauth_client).await?;
        self.try_request(request).await
      }
    }
  }

  async fn try_request(&self, request: reqwest::RequestBuilder) -> OAuthResult<Response> {
    request
      .bearer_auth(self.access_token.clone())
      .send()
      .await?
      .error_for_status()
      .map_err(|err| OAuthError::BadStatus(err.status().unwrap_or(StatusCode::UNAUTHORIZED)))
  }
}

#[derive(Error, Debug)]
pub enum OAuthError {
  #[error(transparent)]
  Exchange(#[from] AsyncRequestError),
  #[error(transparent)]
  Request(#[from] reqwest::Error),
  #[error("Bad request status: {0}")]
  BadStatus(StatusCode),
  #[error("Could not handle request body")]
  InvalidRequestBody,
  #[error("Missing refresh token")]
  NoRefreshToken,
}

type OAuthResult<T> = Result<T, OAuthError>;
