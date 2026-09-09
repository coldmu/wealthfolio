//! The read-only Namu contract.
//!
//! The three query methods below are the *complete* public surface of the
//! adapter. There is deliberately no order method and no order-path constant:
//!
//! ```compile_fail
//! // Order methods are prohibited on the read-only Namu contract. If a
//! // `place_order` method is ever added to `NamuReadService`, this doctest
//! // starts compiling and the test suite fails.
//! fn place_order<T: NamuReadService>(service: &T) {
//!     service.place_order();
//! }
//! ```

use async_trait::async_trait;

use super::error::NamuReadError;
use super::models::{NamuAccount, NamuHolding, NamuQuote};

/// Read-only access to NAMUH PLUG accounts, holdings, and quotes.
///
/// Implementations must never call order, modification, cancellation, or
/// reservation endpoints.
#[async_trait]
pub trait NamuReadService: Send + Sync {
    /// Lists the brokerage accounts reachable with the configured
    /// credentials.
    async fn list_accounts(&self) -> Result<Vec<NamuAccount>, NamuReadError>;

    /// Lists the ETF/stock holdings of `account_id` (zero-quantity rows
    /// omitted).
    async fn list_holdings(&self, account_id: &str) -> Result<Vec<NamuHolding>, NamuReadError>;

    /// Fetches the current market price of `symbol`.
    async fn etf_quote(&self, symbol: &str) -> Result<NamuQuote, NamuReadError>;
}
