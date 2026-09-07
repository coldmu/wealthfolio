//! NAMUH PLUG (나무증권) read-only adapter.
//!
//! This module implements the read side of the Namu spike: OAuth token
//! issuance, account list, domestic holdings/balance, and domestic quotes.
//! It never calls order endpoints — see [`service::NamuReadService`].
//!
//! Wire reference: NH PLUG 공통 OpenAPI (`v1`); request/response envelope
//! `rsp_cd`/`rsp_msg` + `Output_0`/`Output_1`. Field names are confirmed by
//! the live probe (`apps/server/src/bin/verify_namu_account.rs`).

pub mod error;
pub mod models;
pub mod service;

pub use error::NamuReadError;
pub use models::{
    normalize_holding, NamuAccount, NamuAccountsResponse, NamuBalanceResponse, NamuCredentials,
    NamuHolding, NamuQuote, SecretString,
};
pub use service::NamuReadService;

/// Official NAMUH PLUG REST paths used by this adapter. Read-only by
/// construction: none of these segments match the order endpoints.
pub mod paths {
    /// OAuth access-token issuance.
    pub const TOKEN: &str = "/oauth2/token";
    /// Account list.
    pub const ACCOUNT_LIST: &str = "/n2/acctinfo";
    /// Domestic stock/ETF balance (holdings).
    pub const HOLDINGS: &str = "/krstock/inquiry/v1/balance";
    /// Domestic stock/ETF current price.
    pub const QUOTE: &str = "/krstock/quote/v1/currentPrice";
}
