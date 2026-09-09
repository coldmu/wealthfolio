//! Namu (NAMUH PLUG) BrokerApiClient implementation.
//!
//! This wraps the read-only NamuHttpClient to provide the BrokerApiClient
//! interface expected by the sync orchestrator.

use async_trait::async_trait;

use crate::broker::namu::{NamuCredentials, NamuHttpClient, NamuReadService};
use crate::broker::{
    BrokerAccount, BrokerApiClient, BrokerBrokerage, BrokerConnection, BrokerConnectionBrokerage,
    BrokerHoldingsResponse, BrokerTrackingMode, HoldingsBalance, HoldingsCurrency,
    HoldingsExchange, HoldingsInnerSymbol, HoldingsPosition, HoldingsSymbol, HoldingsSymbolType,
    PaginatedUniversalActivity, PaginationDetails,
};
use wealthfolio_core::errors::{Error, Result};

/// Secret keys for Namu credentials in the secret store.
pub const NAMU_API_BASE_URL_KEY: &str = "namu_api_base_url";
pub const NAMU_APP_KEY_KEY: &str = "namu_app_key";
pub const NAMU_APP_SECRET_KEY: &str = "namu_app_secret";

/// Namu brokerage slug.
const NAMU_SLUG: &str = "NAMU";
const NAMU_DISPLAY_NAME: &str = "나무증권";

/// BrokerApiClient implementation for Namu (나무증권).
#[derive(Clone)]
pub struct NamuBrokerApiClient {
    http_client: NamuHttpClient,
}

impl NamuBrokerApiClient {
    /// Creates a new NamuBrokerApiClient from credentials.
    pub fn new(base_url: &str, app_key: &str, app_secret: &str) -> Result<Self> {
        let url = base_url
            .parse()
            .map_err(|e| Error::Unexpected(format!("Invalid NAMU_API_BASE_URL: {}", e)))?;
        let credentials = NamuCredentials::new(app_key, app_secret);
        let http_client = NamuHttpClient::new(url, credentials)
            .map_err(|e| Error::Unexpected(format!("Failed to create Namu HTTP client: {}", e)))?;
        Ok(Self { http_client })
    }

    /// Creates a client from the secret store.
    pub async fn from_secret_store(
        secret_store: &dyn wealthfolio_core::secrets::SecretStore,
    ) -> Result<Self> {
        let base_url = secret_store
            .get_secret(NAMU_API_BASE_URL_KEY)?
            .ok_or_else(|| {
                Error::Unexpected("NAMU_API_BASE_URL not configured in secret store".into())
            })?;
        let app_key = secret_store.get_secret(NAMU_APP_KEY_KEY)?.ok_or_else(|| {
            Error::Unexpected("NAMU_APP_KEY not configured in secret store".into())
        })?;
        let app_secret = secret_store
            .get_secret(NAMU_APP_SECRET_KEY)?
            .ok_or_else(|| {
                Error::Unexpected("NAMU_APP_SECRET not configured in secret store".into())
            })?;

        Self::new(&base_url, &app_key, &app_secret)
    }

    /// Converts a NamuAccount to a BrokerAccount.
    fn namu_account_to_broker_account(
        &self,
        account: &crate::broker::namu::NamuAccount,
    ) -> BrokerAccount {
        BrokerAccount {
            id: Some(account.id.clone()),
            name: account.name.clone().or_else(|| {
                Some(format!(
                    "나무증권 연금계좌 {}",
                    &account.id[account.id.len().saturating_sub(4)..]
                ))
            }),
            account_number: Some(account.id.clone()),
            account_type: Some("PENSION".to_string()),
            currency: Some("KRW".to_string()),
            balance: None,
            meta: None,
            owner: None,
            brokerage_authorization: Some("namu".to_string()),
            institution_name: Some(NAMU_DISPLAY_NAME.to_string()),
            created_date: None,
            sync_status: None,
            status: Some("open".to_string()),
            raw_type: Some("PENSION".to_string()),
            is_paper: false,
            sync_enabled: true,
            shared_with_household: false,
        }
    }

    /// Converts NamuHolding to BrokerHoldingsResponse.
    fn namu_holdings_to_broker_response(
        &self,
        holdings: Vec<crate::broker::namu::NamuHolding>,
    ) -> BrokerHoldingsResponse {
        let positions: Vec<HoldingsPosition> = holdings
            .into_iter()
            .map(|h| HoldingsPosition {
                symbol: Some(HoldingsSymbol {
                    symbol: Some(HoldingsInnerSymbol {
                        id: Some(h.symbol.clone()),
                        symbol: Some(h.symbol.clone()),
                        raw_symbol: Some(h.symbol.clone()),
                        description: None,
                        name: None,
                        currency: Some(HoldingsCurrency {
                            id: Some("KRW".to_string()),
                            code: Some("KRW".to_string()),
                            name: Some("Korean Won".to_string()),
                        }),
                        symbol_type: Some(HoldingsSymbolType {
                            id: Some("ETF".to_string()),
                            code: Some("ETF".to_string()),
                            description: Some("Exchange Traded Fund".to_string()),
                        }),
                        exchange: Some(HoldingsExchange {
                            id: Some("KRX".to_string()),
                            code: Some("KRX".to_string()),
                            mic_code: Some("KRX".to_string()),
                            name: Some("Korea Exchange".to_string()),
                            suffix: None,
                        }),
                    }),
                    id: Some(h.symbol.clone()),
                    description: None,
                }),
                units: Some(h.quantity.try_into().unwrap_or(0.0)),
                price: Some(h.market_price.try_into().unwrap_or(0.0)),
                open_pnl: Some(
                    (h.market_value - h.quantity * h.average_price)
                        .try_into()
                        .unwrap_or(0.0),
                ),
                average_purchase_price: Some(h.average_price.try_into().unwrap_or(0.0)),
                currency: Some(HoldingsCurrency {
                    id: Some("KRW".to_string()),
                    code: Some("KRW".to_string()),
                    name: Some("Korean Won".to_string()),
                }),
                contract_multiplier: Some(1.0),
                cash_equivalent: Some(false),
            })
            .collect();

        BrokerHoldingsResponse {
            account: None,
            balances: Some(vec![HoldingsBalance {
                currency: Some(HoldingsCurrency {
                    id: Some("KRW".to_string()),
                    code: Some("KRW".to_string()),
                    name: Some("Korean Won".to_string()),
                }),
                cash: Some(0.0),
                buying_power: Some(0.0),
            }]),
            positions: Some(positions),
            option_positions: Some(vec![]),
        }
    }
}

#[async_trait]
impl BrokerApiClient for NamuBrokerApiClient {
    async fn list_connections(&self) -> Result<Vec<BrokerConnection>> {
        Ok(vec![BrokerConnection {
            id: "namu".to_string(),
            brokerage: Some(BrokerConnectionBrokerage {
                id: Some("namu".to_string()),
                slug: Some(NAMU_SLUG.to_string()),
                name: Some(NAMU_DISPLAY_NAME.to_string()),
                display_name: Some(NAMU_DISPLAY_NAME.to_string()),
                aws_s3_logo_url: None,
                aws_s3_square_logo_url: None,
            }),
            connection_type: Some("read".to_string()),
            status: Some("connected".to_string()),
            disabled: false,
            disabled_date: None,
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            name: Some(NAMU_DISPLAY_NAME.to_string()),
        }])
    }

    async fn list_accounts(
        &self,
        _authorization_ids: Option<Vec<String>>,
    ) -> Result<Vec<BrokerAccount>> {
        let accounts = self
            .http_client
            .list_accounts()
            .await
            .map_err(|e| Error::Unexpected(format!("Failed to list Namu accounts: {}", e)))?;

        Ok(accounts
            .into_iter()
            .map(|acc| self.namu_account_to_broker_account(&acc))
            .collect())
    }

    async fn list_brokerages(&self) -> Result<Vec<BrokerBrokerage>> {
        Ok(vec![BrokerBrokerage {
            id: Some("namu".to_string()),
            slug: Some(NAMU_SLUG.to_string()),
            name: Some(NAMU_DISPLAY_NAME.to_string()),
            display_name: Some(NAMU_DISPLAY_NAME.to_string()),
            url: Some("https://www.namuh.com".to_string()),
            enabled: true,
        }])
    }

    async fn get_account_activities(
        &self,
        _account_id: &str,
        _tracking_mode: BrokerTrackingMode,
        _start_date: Option<&str>,
        _end_date: Option<&str>,
        _offset: Option<i64>,
        _limit: Option<i64>,
    ) -> Result<PaginatedUniversalActivity> {
        // Namu adapter is read-only for holdings/quotes only.
        // No activity/transaction data available.
        Ok(PaginatedUniversalActivity {
            data: vec![],
            pagination: Some(PaginationDetails {
                offset: Some(0),
                limit: Some(0),
                total: Some(0),
                has_more: Some(false),
            }),
        })
    }

    async fn get_account_holdings(
        &self,
        account_id: &str,
        _tracking_mode: BrokerTrackingMode,
    ) -> Result<BrokerHoldingsResponse> {
        let holdings = self
            .http_client
            .list_holdings(account_id)
            .await
            .map_err(|e| Error::Unexpected(format!("Failed to list Namu holdings: {}", e)))?;

        Ok(self.namu_holdings_to_broker_response(holdings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::namu::{NamuAccount, NamuHolding};
    use rust_decimal::Decimal;

    #[test]
    fn test_namu_account_to_broker_account() {
        let client = NamuBrokerApiClient {
            http_client: NamuHttpClient::new(
                "http://127.0.0.1:1234".parse().unwrap(),
                NamuCredentials::new("test", "test"),
            )
            .unwrap(),
        };

        let account = NamuAccount {
            id: "12345678".to_string(),
            name: None,
        };

        let broker_account = client.namu_account_to_broker_account(&account);
        assert_eq!(broker_account.id, Some("12345678".to_string()));
        assert_eq!(broker_account.account_type, Some("PENSION".to_string()));
        assert_eq!(broker_account.currency, Some("KRW".to_string()));
    }

    #[test]
    fn test_namu_holdings_to_broker_response() {
        let client = NamuBrokerApiClient {
            http_client: NamuHttpClient::new(
                "http://127.0.0.1:1234".parse().unwrap(),
                NamuCredentials::new("test", "test"),
            )
            .unwrap(),
        };

        let holdings = vec![NamuHolding {
            symbol: "360750".to_string(),
            quantity: Decimal::from(5),
            average_price: Decimal::from(28500),
            market_price: Decimal::from(30120),
            market_value: Decimal::from(150600),
        }];

        let response = client.namu_holdings_to_broker_response(holdings);
        assert_eq!(response.positions.as_ref().unwrap().len(), 1);
        let pos = &response.positions.as_ref().unwrap()[0];
        assert_eq!(pos.units, Some(5.0));
        assert_eq!(pos.price, Some(30120.0));
        assert_eq!(pos.average_purchase_price, Some(28500.0));
    }
}
