//! Read-only error taxonomy for the Namu (NAMUH PLUG) adapter.
//!
//! Deliberately has **no order-related variant**: this adapter only calls
//! read endpoints. Every variant is display-safe — credentials and response
//! bodies are stripped at the transport boundary.

use std::fmt;

/// Errors returned by read-only Namu (NAMUH PLUG) calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamuReadError {
    /// Token issuance or validation failed (HTTP 401, or a documented
    /// auth gateway code such as `IGW40031`/`IGW40032`/`IGW40037`/
    /// `IGW40043`/`IGW40044`).
    Unauthorized,
    /// The target account is not covered by the read APIs. For pension
    /// accounts this is unproven until the live probe in
    /// `apps/server/src/bin/verify_namu_account.rs` confirms coverage.
    AccountUnsupported,
    /// The response was a business failure or was missing/invalid mandatory
    /// fields. Carries a short human-readable reason only.
    IncompleteResponse(String),
    /// Transport-level failure. Carries the HTTP status *only* — never a
    /// response body.
    Transport(String),
}

impl fmt::Display for NamuReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => formatter.write_str("Namu authentication failed"),
            Self::AccountUnsupported => {
                formatter.write_str("account is not supported by the read APIs")
            }
            Self::IncompleteResponse(reason) => {
                write!(formatter, "incomplete Namu response: {reason}")
            }
            Self::Transport(status) => write!(formatter, "Namu transport error (HTTP {status})"),
        }
    }
}

impl std::error::Error for NamuReadError {}

/// Documented gateway codes that mean the token/credentials are not usable.
/// Source: NH PLUG OpenAPI error reference (IGW40031 invalid AppKey,
/// IGW40032 invalid AppSecret, IGW40037 invalid grant, IGW40043 invalid
/// token, IGW40044 expired token).
pub(crate) const AUTH_FAILURE_CODES: &[&str] =
    &["IGW40031", "IGW40032", "IGW40037", "IGW40043", "IGW40044"];

/// Business codes treated as success (from the official sample server's
/// default set plus the documented `00000`). `00166` is returned by the
/// account-list call, `00221`/`13578` by some market-data queries.
pub(crate) const SUCCESS_CODES: &[&str] = &["00000", "00166", "00221", "13578"];

/// Message fragments that identify an account the read APIs cannot serve
/// (pension accounts are the suspected case). Confirmed/extended by the
/// live probe in Task 4.
pub(crate) const UNSUPPORTED_ACCOUNT_MARKERS: &[&str] = &["연금", "퇴직연금", "퇴직"];

/// Classifies a business-failure envelope (`rsp_cd` outside the success
/// set) into [`NamuReadError`]. Returns `None` when the code is a success
/// code.
pub(crate) fn classify_business_failure(
    rsp_cd: Option<&str>,
    rsp_msg: Option<&str>,
) -> Option<NamuReadError> {
    let code = rsp_cd.unwrap_or_default();
    if SUCCESS_CODES.contains(&code) {
        return None;
    }
    if AUTH_FAILURE_CODES.contains(&code) {
        return Some(NamuReadError::Unauthorized);
    }
    let message = rsp_msg.unwrap_or_default();
    if UNSUPPORTED_ACCOUNT_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        return Some(NamuReadError::AccountUnsupported);
    }
    Some(NamuReadError::IncompleteResponse(if message.is_empty() {
        format!("business error code {code}")
    } else {
        format!("{code}: {message}")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_codes_map_to_ok() {
        assert_eq!(
            classify_business_failure(Some("00000"), Some("Success")),
            None
        );
    }

    #[test]
    fn pension_marker_maps_to_account_unsupported() {
        assert_eq!(
            classify_business_failure(
                Some("20012"),
                Some("연금계좌는 조회 업무가 지원되지 않습니다")
            ),
            Some(NamuReadError::AccountUnsupported)
        );
    }

    #[test]
    fn invalid_token_code_maps_to_unauthorized() {
        assert_eq!(
            classify_business_failure(Some("IGW40044"), Some("만료된 token")),
            Some(NamuReadError::Unauthorized)
        );
    }
}
