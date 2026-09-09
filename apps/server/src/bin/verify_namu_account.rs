//! No-write NAMUH PLUG pension-account eligibility probe.
//!
//! Proves whether the read-only Namu adapter can reach the target account.
//! This binary never writes to the Wealthfolio database (no SQLite write
//! transaction is ever opened) and never calls an order endpoint.
//!
//! Fixture mode (no network):
//! ```text
//! cargo run -p wealthfolio-server --bin verify_namu_account -- --account-id 12345678
//! ```
//!
//! Live mode (uses `NAMU_API_BASE_URL`, `NAMU_APP_KEY`, `NAMU_APP_SECRET`,
//! `NAMU_ACCOUNT_ID` from the environment — populated by the operator from
//! the encrypted secrets store, never committed):
//! ```text
//! cargo run -p wealthfolio-server --bin verify_namu_account -- --live --account-id <id>
//! ```
//!
//! Exit codes: `0` supported, `1` unsupported/unauthorized/incomplete,
//! `2` transport or usage error.

use std::env;
use std::process::ExitCode;

use wealthfolio_connect::broker::namu::{
    NamuAccount, NamuCredentials, NamuHttpClient, NamuReadError, NamuReadService,
};

/// Fixture account id used in fixture mode (matches `accounts.json`).
const FIXTURE_ACCOUNT_ID: &str = "12345678";

/// Masks an account id, keeping only the last four characters.
fn mask_account_id(id: &str) -> String {
    if id.len() <= 4 {
        return "*".repeat(id.len());
    }
    format!("{}****{}", "*".repeat(id.len() - 4), &id[id.len() - 4..])
}

/// Renders a supported/unsupported result with a masked account id.
fn render_result(account: &NamuAccount, supported: bool, holdings: usize) -> String {
    format!(
        "Namu account {} -> {} ({} holdings)",
        mask_account_id(&account.id),
        if supported {
            "Supported"
        } else {
            "Unsupported"
        },
        holdings
    )
}

/// Renders the unsupported-account outcome (e.g. pension accounts).
fn render_unsupported(account_id: &str) -> String {
    format!(
        "Namu account {} -> Unsupported: confirm pension-account coverage with NAMUH PLUG",
        mask_account_id(account_id)
    )
}

fn exit_code_for(error: &NamuReadError) -> ExitCode {
    match error {
        NamuReadError::Unauthorized
        | NamuReadError::AccountUnsupported
        | NamuReadError::IncompleteResponse(_) => ExitCode::from(1),
        NamuReadError::Transport(_) => ExitCode::from(2),
    }
}

fn parse_args() -> Result<(bool, Option<String>), String> {
    let mut live = false;
    let mut account_id = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--live" => live = true,
            "--account-id" => {
                account_id = Some(
                    args.next()
                        .ok_or_else(|| "--account-id requires a value".to_owned())?,
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((live, account_id))
}

/// Picks the account to probe: an explicit `--account-id` wins, then
/// `NAMU_ACCOUNT_ID` from the environment, then the first listed account.
fn pick_account(
    accounts: Vec<NamuAccount>,
    requested: Option<&str>,
) -> Result<NamuAccount, NamuReadError> {
    match requested {
        Some(id) => accounts
            .into_iter()
            .find(|account| account.id == id)
            .ok_or_else(|| {
                // Mask the id — full account numbers must not leak into logs.
                NamuReadError::IncompleteResponse(format!(
                    "requested account {} not found in account list",
                    mask_account_id(id)
                ))
            }),
        None => accounts
            .into_iter()
            .next()
            .ok_or_else(|| NamuReadError::IncompleteResponse("no accounts returned".into())),
    }
}

async fn run_live(cli_account_id: Option<&str>) -> Result<(NamuAccount, usize), NamuReadError> {
    let base_url = env::var("NAMU_API_BASE_URL")
        .map_err(|_| NamuReadError::IncompleteResponse("NAMU_API_BASE_URL is not set".into()))?;
    let app_key = env::var("NAMU_APP_KEY")
        .map_err(|_| NamuReadError::IncompleteResponse("NAMU_APP_KEY is not set".into()))?;
    let app_secret = env::var("NAMU_APP_SECRET")
        .map_err(|_| NamuReadError::IncompleteResponse("NAMU_APP_SECRET is not set".into()))?;
    let configured = env::var("NAMU_ACCOUNT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let url = base_url.parse().map_err(|_| {
        NamuReadError::IncompleteResponse("NAMU_API_BASE_URL is not a valid URL".into())
    })?;
    let client = NamuHttpClient::new(url, NamuCredentials::new(app_key, app_secret))?;

    let accounts = client.list_accounts().await?;
    let requested = cli_account_id.or(configured.as_deref());
    let target = pick_account(accounts, requested)?;

    let holdings = client.list_holdings(&target.id).await?;
    Ok((target, holdings.len()))
}

#[tokio::main]
async fn main() -> ExitCode {
    let (live, account_id) = match parse_args() {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    if !live {
        let account = NamuAccount {
            id: FIXTURE_ACCOUNT_ID.to_owned(),
            name: None,
        };
        println!("{}", render_result(&account, true, 1));
        return ExitCode::SUCCESS;
    }

    match run_live(account_id.as_deref()).await {
        Ok((account, holdings)) => {
            println!("{}", render_result(&account, true, holdings));
            ExitCode::SUCCESS
        }
        Err(NamuReadError::AccountUnsupported) => {
            let id = account_id.unwrap_or_default();
            println!("{}", render_unsupported(&id));
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("{error}");
            exit_code_for(&error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_result_masks_account_id() {
        let account = NamuAccount {
            id: "12345678".to_owned(),
            name: None,
        };
        let output = render_result(&account, true, 1);
        assert!(output.contains("****5678"), "got: {output}");
        assert!(!output.contains("12345678"), "got: {output}");
    }

    #[test]
    fn render_unsupported_mentions_pension_coverage() {
        let output = render_unsupported("12345678");
        assert!(
            output.contains("confirm pension-account coverage with NAMUH PLUG"),
            "got: {output}"
        );
        assert!(!output.contains("12345678"), "got: {output}");
    }

    #[test]
    fn short_account_ids_are_fully_masked() {
        assert_eq!(mask_account_id("1234"), "****");
    }

    #[test]
    fn pick_account_prefers_cli_then_first() {
        let accounts = vec![
            NamuAccount {
                id: "11111111".to_owned(),
                name: None,
            },
            NamuAccount {
                id: "22222222".to_owned(),
                name: None,
            },
        ];

        // An explicitly requested id wins.
        let picked = pick_account(accounts.clone(), Some("22222222")).unwrap();
        assert_eq!(picked.id, "22222222");

        // Nothing requested -> first account.
        let picked = pick_account(accounts.clone(), None).unwrap();
        assert_eq!(picked.id, "11111111");

        // A not-found error keeps the account id masked.
        let error = pick_account(accounts, Some("99999999")).unwrap_err();
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("****9999"),
            "error must mask the account id: {rendered}"
        );
        assert!(
            !rendered.contains("99999999"),
            "full account number must not appear: {rendered}"
        );
    }
}
