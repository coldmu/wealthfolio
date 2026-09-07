//! NAMUH PLUG HTTP transport (OAuth + read-only JSON calls).
//!
//! Security rules enforced here:
//! - HTTPS is required except for loopback hosts (fixture tests).
//! - Credentials are never logged; `Debug` redacts them.
//! - Errors carry an HTTP status only — never a response body.
//! - Only the read paths from [`super::paths`] are ever called.

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::error::{classify_business_failure, NamuReadError};
use super::models::{
    normalize_account, normalize_holding, normalize_quote, retain_nonzero_holdings,
    NamuAccountsResponse, NamuBalanceResponse, NamuCredentials, NamuQuoteResponse,
    NamuTokenResponse, SecretString,
};
use super::paths;
use super::service::NamuReadService;

/// Minimal transport error text — never includes URLs or bodies.
fn transport_message(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "request timed out"
    } else if error.is_connect() {
        "could not connect to the server"
    } else {
        "request failed"
    }
}

/// Cloneable, read-only NAMUH PLUG HTTP client.
#[derive(Clone)]
pub struct NamuHttpClient {
    http: reqwest::Client,
    base_url: Url,
    credentials: NamuCredentials,
}

impl std::fmt::Debug for NamuHttpClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NamuHttpClient")
            .field("base_url", &self.base_url.as_str())
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

impl NamuHttpClient {
    /// Builds a client. `base_url` must be HTTPS unless it is a loopback
    /// address (used by fixture tests against a local mock server).
    pub fn new(base_url: Url, credentials: NamuCredentials) -> Result<Self, NamuReadError> {
        let is_loopback = base_url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
        if base_url.scheme() != "https" && !is_loopback {
            return Err(NamuReadError::Transport(
                "HTTPS is required for the NAMUH PLUG API".to_owned(),
            ));
        }
        let http = reqwest::Client::builder()
            .build()
            .map_err(|error| NamuReadError::Transport(error.to_string()))?;
        Ok(Self {
            http,
            base_url,
            credentials,
        })
    }

    /// Issues an OAuth access token using the official NAMUH PLUG token
    /// endpoint (`POST /oauth2/token` with `appkey`, `appsecretkey`,
    /// `grant_type=client_credentials`, `scope=oob`).
    pub async fn access_token(
        &self,
        credentials: &NamuCredentials,
    ) -> Result<SecretString, NamuReadError> {
        let url = self
            .base_url
            .join(paths::TOKEN)
            .map_err(|error| NamuReadError::Transport(error.to_string()))?;
        let response = self
            .http
            .post(url)
            .query(&[
                ("appkey", credentials.app_key().as_str()),
                ("appsecretkey", credentials.app_secret().as_str()),
                ("grant_type", "client_credentials"),
                ("scope", "oob"),
            ])
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body("")
            .send()
            .await
            .map_err(|error| NamuReadError::Transport(transport_message(&error).to_owned()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|_| NamuReadError::Transport(status.as_str().to_owned()))?;

        if status != reqwest::StatusCode::OK {
            return Err(NamuReadError::Unauthorized);
        }
        let parsed: NamuTokenResponse =
            serde_json::from_str(&text).map_err(|_| NamuReadError::Unauthorized)?;
        if parsed.access_token.is_empty() {
            return Err(NamuReadError::Unauthorized);
        }
        Ok(SecretString::new(parsed.access_token))
    }

    /// GETs `path` with the bearer token and the `x-client-id` /
    /// `x-client-secret` headers, decodes the business envelope, and
    /// deserializes the payload.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        token: &SecretString,
    ) -> Result<T, NamuReadError> {
        self.request_json(Method::GET, path, None, token).await
    }

    /// POSTs `body` as JSON to `path` and decodes the payload like
    /// [`Self::get_json`].
    pub async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Value,
        token: &SecretString,
    ) -> Result<T, NamuReadError> {
        self.request_json(Method::POST, path, Some(body), token)
            .await
    }

    async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: &SecretString,
    ) -> Result<T, NamuReadError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|error| NamuReadError::Transport(error.to_string()))?;

        let mut request = self
            .http
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {}", token.as_str()))
            .header("x-client-id", self.credentials.app_key().as_str())
            .header("x-client-secret", self.credentials.app_secret().as_str())
            .header(CONTENT_TYPE, "application/json; charset=UTF-8");
        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .await
            .map_err(|error| NamuReadError::Transport(transport_message(&error).to_owned()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|_| NamuReadError::Transport(status.as_str().to_owned()))?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(NamuReadError::Unauthorized);
        }
        if !status.is_success() {
            // Status only — never the body.
            return Err(NamuReadError::Transport(status.as_str().to_owned()));
        }

        let value: Value = serde_json::from_str(&text).map_err(|_| {
            NamuReadError::IncompleteResponse("response is not valid JSON".to_owned())
        })?;
        let rsp_cd = value.get("rsp_cd").and_then(Value::as_str);
        let rsp_msg = value.get("rsp_msg").and_then(Value::as_str);
        if let Some(error) = classify_business_failure(rsp_cd, rsp_msg) {
            return Err(error);
        }
        serde_json::from_value(value)
            .map_err(|error| NamuReadError::IncompleteResponse(error.to_string()))
    }
}

#[async_trait]
impl NamuReadService for NamuHttpClient {
    async fn list_accounts(&self) -> Result<Vec<super::models::NamuAccount>, NamuReadError> {
        let token = self.access_token(&self.credentials).await?;
        let response: NamuAccountsResponse = self.get_json(paths::ACCOUNT_LIST, &token).await?;
        response
            .accounts
            .iter()
            .map(normalize_account)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn list_holdings(
        &self,
        account_id: &str,
    ) -> Result<Vec<super::models::NamuHolding>, NamuReadError> {
        let token = self.access_token(&self.credentials).await?;
        let body = json!({
            "act_no": account_id,
            "bnc_bse_cd": "1",
            "ltg_aot_dit_cd": "1",
            "aet_bse": "1",
            "qut_dit_cd": "UNT",
        });
        let response: NamuBalanceResponse = self.post_json(paths::HOLDINGS, body, &token).await?;
        let holdings = response
            .holdings
            .iter()
            .map(normalize_holding)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(retain_nonzero_holdings(holdings))
    }

    async fn etf_quote(&self, symbol: &str) -> Result<super::models::NamuQuote, NamuReadError> {
        let token = self.access_token(&self.credentials).await?;
        let body = json!({
            "iem_cd": symbol,
            "market_cd": "KRX",
        });
        let response: NamuQuoteResponse = self.post_json(paths::QUOTE, body, &token).await?;
        let raw = response.quote.ok_or_else(|| {
            NamuReadError::IncompleteResponse("quote Output_0 is missing".to_owned())
        })?;
        normalize_quote(&raw)
    }
}
