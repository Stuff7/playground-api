use oauth2::{
  basic::{BasicClient, BasicErrorResponseType, BasicTokenType},
  reqwest::async_http_client,
  AuthorizationCode, EmptyExtraTokenFields, RequestTokenError, StandardErrorResponse,
  StandardTokenResponse, TokenResponse,
};
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
}

#[derive(Error, Debug)]
pub enum OAuthError {
  #[error(transparent)]
  Exchange(#[from] AsyncRequestError),
  #[error(transparent)]
  Request(#[from] reqwest::Error),
}

type OAuthResult<T> = Result<T, OAuthError>;
