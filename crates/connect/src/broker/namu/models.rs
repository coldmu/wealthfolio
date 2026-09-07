//! Wire (NAMUH PLUG `v1`) DTOs, normalized domain types, and mapping from
//! wire to domain. Field names mirror the official PLUG OpenAPI: requests
//! carry an `Input_0` object, responses an `rsp_cd`/`rsp_msg` envelope with
//! `Output_0` (single) / `Output_1` (row list) blocks.
//!
//! Reference: NH PLUG 공통 OpenAPI + community `nhplug` crate (MIT) which
//! documents the same wire names; live probe (Task 4) is the authority and
//! will update these fixtures if the real response differs.

use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::error::NamuReadError;

// ---------------------------------------------------------------------------
// Secret handling
// ---------------------------------------------------------------------------

/// A credential value that never appears in `Debug`/`Display` output.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    /// Wraps a secret value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Exposes the secret for request construction.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

/// API credentials issued by NAMUH PLUG. Never logged; the verification
/// binary reads them from Wealthfolio's encrypted secret store.
#[derive(Clone)]
pub struct NamuCredentials {
    app_key: SecretString,
    app_secret: SecretString,
}

impl NamuCredentials {
    /// Creates credentials from raw values.
    #[must_use]
    pub fn new(app_key: impl Into<String>, app_secret: impl Into<String>) -> Self {
        Self {
            app_key: SecretString::new(app_key),
            app_secret: SecretString::new(app_secret),
        }
    }

    #[must_use]
    pub(crate) fn app_key(&self) -> &SecretString {
        &self.app_key
    }

    #[must_use]
    pub(crate) fn app_secret(&self) -> &SecretString {
        &self.app_secret
    }
}

// ---------------------------------------------------------------------------
// Normalized domain types (the read-only contract surface)
// ---------------------------------------------------------------------------

/// A brokerage account as returned by the read APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamuAccount {
    /// Broker account number (e.g. `12345678`).
    pub id: String,
    /// Display name when the API provides one.
    pub name: Option<String>,
}

/// A domestic ETF/stock position held in the account.
#[derive(Debug, Clone, PartialEq)]
pub struct NamuHolding {
    /// Instrument code (e.g. `360750` for TIGER 미국S&P500).
    pub symbol: String,
    /// Quantity held (fractional allowed by the wire format).
    pub quantity: Decimal,
    /// Average purchase price.
    pub average_price: Decimal,
    /// Current market price.
    pub market_price: Decimal,
    /// Market value (`evlu_amt`, or quantity × market price when absent).
    pub market_value: Decimal,
}

/// A domestic market quote.
#[derive(Debug, Clone, PartialEq)]
pub struct NamuQuote {
    pub symbol: String,
    pub market_price: Decimal,
    /// `None` until the API's currency field is confirmed by the live probe.
    pub currency: Option<String>,
}

/// OAuth token response.
#[derive(Debug, Deserialize)]
pub struct NamuTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Parse helper: `Option<String>` field that tolerates JSON strings, numbers,
/// and `null` (PLUG returns numeric fields as strings; numbers are accepted
/// defensively).
fn de_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::String(text) => Some(text),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }))
}

/// One row of the `Output_0` list returned by `/n2/acctinfo`.
#[derive(Debug, Default, Deserialize)]
pub struct NamuAccountRaw {
    #[serde(rename = "acct_no", default, deserialize_with = "de_opt_string")]
    pub acct_no: Option<String>,
    #[serde(rename = "acct_type", default)]
    pub acct_type: Option<String>,
    #[serde(rename = "acct_name", alias = "acct_nm", default)]
    pub acct_name: Option<String>,
}

/// Account-list response envelope.
#[derive(Debug, Default, Deserialize)]
pub struct NamuAccountsResponse {
    #[serde(rename = "rsp_cd", default)]
    pub rsp_cd: Option<String>,
    #[serde(rename = "rsp_msg", default)]
    pub rsp_msg: Option<String>,
    #[serde(rename = "Output_0", default)]
    pub accounts: Vec<NamuAccountRaw>,
}

/// One holding row of the `Output_1` list of `/krstock/inquiry/v1/balance`.
#[derive(Debug, Default, Deserialize)]
pub struct NamuHoldingRaw {
    #[serde(rename = "iem_cd", default, deserialize_with = "de_opt_string")]
    pub iem_cd: Option<String>,
    #[serde(rename = "iem_nm", default)]
    pub iem_nm: Option<String>,
    #[serde(rename = "rsdl_qty", default, deserialize_with = "de_opt_string")]
    pub rsdl_qty: Option<String>,
    #[serde(rename = "phs_pr", default, deserialize_with = "de_opt_string")]
    pub phs_pr: Option<String>,
    #[serde(rename = "now_pr", default, deserialize_with = "de_opt_string")]
    pub now_pr: Option<String>,
    #[serde(rename = "evlu_amt", default, deserialize_with = "de_opt_string")]
    pub evlu_amt: Option<String>,
}

/// Balance-inquiry response envelope.
#[derive(Debug, Default, Deserialize)]
pub struct NamuBalanceResponse {
    #[serde(rename = "rsp_cd", default)]
    pub rsp_cd: Option<String>,
    #[serde(rename = "rsp_msg", default)]
    pub rsp_msg: Option<String>,
    #[serde(rename = "Output_1", default)]
    pub holdings: Vec<NamuHoldingRaw>,
}

/// Single-object response envelope, e.g. `Output_0` of the quote call.
#[derive(Debug, Default, Deserialize)]
pub struct NamuQuoteResponse {
    #[serde(rename = "rsp_cd", default)]
    pub rsp_cd: Option<String>,
    #[serde(rename = "rsp_msg", default)]
    pub rsp_msg: Option<String>,
    #[serde(rename = "Output_0", default)]
    pub quote: Option<NamuQuoteRaw>,
}

#[derive(Debug, Default, Deserialize)]
pub struct NamuQuoteRaw {
    #[serde(rename = "iem_cd", default)]
    pub iem_cd: Option<String>,
    #[serde(
        rename = "stck_prpr",
        alias = "now_pr",
        default,
        deserialize_with = "de_opt_string"
    )]
    pub stck_prpr: Option<String>,
}

// ---------------------------------------------------------------------------
// Normalization (wire -> domain)
// ---------------------------------------------------------------------------

/// Parses a mandatory numeric wire string. Absent, empty, or non-numeric
/// values map to [`NamuReadError::IncompleteResponse`].
pub fn parse_amount(raw: Option<String>, description: &str) -> Result<Decimal, NamuReadError> {
    let Some(raw) = raw else {
        return Err(NamuReadError::IncompleteResponse(format!(
            "{description} is missing"
        )));
    };
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect();
    if cleaned.is_empty() {
        return Err(NamuReadError::IncompleteResponse(format!(
            "{description} is empty"
        )));
    }
    cleaned
        .parse::<Decimal>()
        .map_err(|_| NamuReadError::IncompleteResponse(format!("{description} is not numeric")))
}

fn require_string(raw: &Option<String>, description: &str) -> Result<String, NamuReadError> {
    raw.clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| NamuReadError::IncompleteResponse(format!("{description} is missing")))
}

/// Maps one balance row to a [`NamuHolding`]. All five fields are mandatory;
/// `evlu_amt` falls back to `quantity × market_price` when absent.
pub fn normalize_holding(raw: &NamuHoldingRaw) -> Result<NamuHolding, NamuReadError> {
    let symbol = require_string(&raw.iem_cd, "iem_cd")?;
    let quantity = parse_amount(raw.rsdl_qty.clone(), "rsdl_qty")?;
    let average_price = parse_amount(raw.phs_pr.clone(), "phs_pr")?;
    let market_price = parse_amount(raw.now_pr.clone(), "now_pr")?;
    let market_value = match raw.evlu_amt.clone() {
        Some(value) => parse_amount(Some(value), "evlu_amt")?,
        None => quantity * market_price,
    };
    Ok(NamuHolding {
        symbol,
        quantity,
        average_price,
        market_price,
        market_value,
    })
}

/// Maps a quote row to a [`NamuQuote`].
pub fn normalize_quote(raw: &NamuQuoteRaw) -> Result<NamuQuote, NamuReadError> {
    let symbol = require_string(&raw.iem_cd, "iem_cd")?;
    let market_price = parse_amount(raw.stck_prpr.clone(), "stck_prpr")?;
    Ok(NamuQuote {
        symbol,
        market_price,
        currency: None,
    })
}

/// Maps an account row to a [`NamuAccount`]. `acct_no` is mandatory.
pub fn normalize_account(raw: &NamuAccountRaw) -> Result<NamuAccount, NamuReadError> {
    let id = require_string(&raw.acct_no, "acct_no")?;
    Ok(NamuAccount {
        id,
        name: raw.acct_name.clone(),
    })
}

/// Drops rows with zero quantity (ETF positions with `rsdl_qty == 0`).
pub fn retain_nonzero_holdings(holdings: Vec<NamuHolding>) -> Vec<NamuHolding> {
    holdings
        .into_iter()
        .filter(|holding| !holding.quantity.is_zero())
        .collect()
}

impl fmt::Debug for NamuCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NamuCredentials { app_key: [REDACTED], app_secret: [REDACTED] }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_amount_accepts_commas_and_whitespace() {
        assert_eq!(
            parse_amount(Some("1,234.50".to_owned()), "x").unwrap(),
            Decimal::from_str_exact("1234.50").unwrap()
        );
    }

    #[test]
    fn parse_amount_rejects_missing_and_garbage() {
        assert!(matches!(
            parse_amount(None, "x"),
            Err(NamuReadError::IncompleteResponse(_))
        ));
        assert!(matches!(
            parse_amount(Some("abc".to_owned()), "x"),
            Err(NamuReadError::IncompleteResponse(_))
        ));
    }

    #[test]
    fn normalize_holding_falls_back_to_qty_times_price() {
        let raw = NamuHoldingRaw {
            iem_cd: Some("360750".to_owned()),
            rsdl_qty: Some("5".to_owned()),
            phs_pr: Some("28500".to_owned()),
            now_pr: Some("30120".to_owned()),
            ..Default::default()
        };
        let holding = normalize_holding(&raw).unwrap();
        assert_eq!(holding.market_value, Decimal::from(150_600));
    }

    #[test]
    fn retain_nonzero_holdings_filters_zero_quantity() {
        let zero = NamuHoldingRaw {
            iem_cd: Some("000000".to_owned()),
            rsdl_qty: Some("0".to_owned()),
            phs_pr: Some("1".to_owned()),
            now_pr: Some("1".to_owned()),
            ..Default::default()
        };
        let one = NamuHoldingRaw {
            iem_cd: Some("360750".to_owned()),
            rsdl_qty: Some("5".to_owned()),
            phs_pr: Some("1".to_owned()),
            now_pr: Some("1".to_owned()),
            ..Default::default()
        };
        let holdings = vec![
            normalize_holding(&zero).unwrap(),
            normalize_holding(&one).unwrap(),
        ];
        let kept = retain_nonzero_holdings(holdings);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].symbol, "360750");
    }
}
