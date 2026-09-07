//! Read-only Namu adapter tests.
//!
//! Covers: fixture normalization (all five `NamuHolding` fields), the
//! pension-account error mapping, the compile-time "no order method" guard,
//! and a request-recording mock server proving that only read endpoints are
//! ever called.

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use wealthfolio_connect::broker::namu::{
    normalize_holding, NamuBalanceResponse, NamuCredentials, NamuHolding, NamuHttpClient,
    NamuReadError, NamuReadService,
};

const FIXTURE_DIR: &str = "tests/fixtures/namu";

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(FIXTURE_DIR)
            .join(name),
    )
    .unwrap_or_else(|error| panic!("failed to read fixture {name}: {error}"))
}

fn test_credentials() -> NamuCredentials {
    NamuCredentials::new("fixture-app-key", "fixture-app-secret")
}

// ---------------------------------------------------------------------------
// Request-recording mock server
// ---------------------------------------------------------------------------

/// A recorded request: method, request target (path + query), and header
/// *names* only ??header values are never recorded (redaction by design).
#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    target: String,
    header_names: Vec<String>,
}

/// Minimal loopback HTTP server that records requests and answers from a
/// path -> (status, body) route table.
struct MockNamuServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl MockNamuServer {
    async fn start(routes: HashMap<String, (u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let routes = Arc::new(routes);

        let requests_for_loop = Arc::clone(&requests);
        let routes_for_loop = Arc::clone(&routes);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let requests = Arc::clone(&requests_for_loop);
                let routes = Arc::clone(&routes_for_loop);
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 16_384];
                    let Ok(read) = socket.read(&mut buffer).await else {
                        return;
                    };
                    let text = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let mut lines = text.lines();
                    let request_line = lines.next().unwrap_or_default().to_string();
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or_default().to_string();
                    let target = parts.next().unwrap_or_default().to_string();
                    let header_names: Vec<String> = lines
                        .take_while(|line| !line.is_empty())
                        .filter_map(|line| line.split(':').next())
                        .map(|name| name.trim().to_ascii_lowercase())
                        .collect();
                    let path_for_route = target.split('?').next().unwrap_or(&target).to_string();
                    requests.lock().unwrap().push(RecordedRequest {
                        method,
                        target,
                        header_names,
                    });

                    let (status, body) = routes
                        .get(&path_for_route)
                        .cloned()
                        .unwrap_or_else(|| (404, "{}".to_string()));
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        Self { addr, requests }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn recorded(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn fixture_routes() -> HashMap<String, (u16, String)> {
    HashMap::from([
        ("/oauth2/token".to_string(), (200, fixture("token.json"))),
        ("/n2/acctinfo".to_string(), (200, fixture("accounts.json"))),
        (
            "/krstock/inquiry/v1/balance".to_string(),
            (200, fixture("holdings.json")),
        ),
        (
            "/krstock/quote/v1/currentPrice".to_string(),
            (200, fixture("etf-quote.json")),
        ),
    ])
}

async fn client_for(server: &MockNamuServer) -> NamuHttpClient {
    NamuHttpClient::new(server.url().parse().unwrap(), test_credentials()).unwrap()
}

// ---------------------------------------------------------------------------
// Task 2: normalization + error mapping + no-order-method guard
// ---------------------------------------------------------------------------

#[test]
fn holdings_response_normalizes_etf_position() {
    let body = fixture("holdings.json");
    let response: NamuBalanceResponse = serde_json::from_str(&body).unwrap();
    assert_eq!(response.rsp_cd.as_deref(), Some("00000"));

    let holdings: Vec<NamuHolding> = response
        .holdings
        .iter()
        .map(normalize_holding)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(holdings.len(), 1);

    let holding = &holdings[0];
    assert_eq!(holding.symbol, "360750");
    assert_eq!(holding.quantity, Decimal::from(5));
    assert_eq!(holding.average_price, Decimal::from(28500));
    assert_eq!(holding.market_price, Decimal::from(30120));
    assert_eq!(holding.market_value, Decimal::from(150600));
}

#[tokio::test]
async fn pension_account_error_maps_to_account_unsupported() {
    let server = MockNamuServer::start(HashMap::from([
        ("/oauth2/token".to_string(), (200, fixture("token.json"))),
        (
            "/krstock/inquiry/v1/balance".to_string(),
            (200, fixture("pension-unsupported.json")),
        ),
    ]))
    .await;
    let client = client_for(&server).await;

    let error = client.list_holdings("12345678").await.unwrap_err();
    assert_eq!(error, NamuReadError::AccountUnsupported);
    assert!(
        format!("{error}").contains("not supported"),
        "error text must be display-safe"
    );
}

#[test]
fn adapter_exposes_no_order_method() {
    // Compile-time guard: the complete public surface is exactly the three
    // query methods below. If a method is removed/renamed this stops
    // compiling; if an order method is added, the `compile_fail` doctest on
    // `NamuReadService` starts failing.
    fn use_read_surface<T: NamuReadService>(service: &T) {
        // The futures are never polled — only their existence is asserted.
        drop(service.list_accounts());
        drop(service.list_holdings("any"));
        drop(service.etf_quote("any"));
    }
    let _ = use_read_surface::<NamuHttpClient>;
}

// ---------------------------------------------------------------------------
// Task 3: OAuth + read-only protocol (request recording)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oauth_and_read_calls_are_read_only() {
    let server = MockNamuServer::start(fixture_routes()).await;
    let client = client_for(&server).await;

    // 1. OAuth token issuance (official path + grant).
    let token = client.access_token(&test_credentials()).await.unwrap();
    assert_eq!(token.as_str(), "access-token-fixture-0123456789");

    // 2. Account list..
    let accounts = client.list_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, "12345678");

    // 3. Holdings (ETF position).
    let holdings = client.list_holdings("12345678").await.unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].symbol, "360750");

    // 4. Quote..
    let quote = client.etf_quote("360750").await.unwrap();
    assert_eq!(quote.market_price, Decimal::from(30120));

    let recorded = server.recorded();
    // Each data call first issues its own OAuth token: 3 token + 3
    // data calls + the explicit token call above = 7 recorded requests.
    assert_eq!(
        recorded.len(),
        7,
        "expected 7 read calls, got {:?}",
        recorded
    );

    // Token call: POST to the official OAuth path with client_credentials..
    let token_call = &recorded[0];
    assert_eq!(token_call.method, "POST");
    assert!(token_call.target.starts_with("/oauth2/token?"));
    assert!(token_call.target.contains("grant_type=client_credentials"));
    assert!(token_call.target.contains("scope=oob"));
    assert!(token_call.target.contains("appkey=fixture-app-key"));

    // Data calls: documented read paths, auth headers attached..
    let expected = [
        (2, "GET", "/n2/acctinfo"),
        (4, "POST", "/krstock/inquiry/v1/balance"),
        (6, "POST", "/krstock/quote/v1/currentPrice"),
    ];
    for (index, method, path) in expected {
        let call = &recorded[index];
        assert_eq!(call.method, method, "method for {path}");
        assert_eq!(
            call.target.split('?').next().unwrap_or(&call.target),
            path,
            "path for {path}"
        );
        assert!(
            call.header_names.iter().any(|name| name == "authorization"),
            "bearer authorization header on {path}"
        );
        assert!(
            call.header_names.iter().any(|name| name == "x-client-id"),
            "x-client-id header on {path}"
        );
        assert!(
            call.header_names
                .iter()
                .any(|name| name == "x-client-secret"),
            "x-client-secret header on {path}"
        );
    }

    // Every recorded request must be one of the four read endpoints — an
    // allowlist invariant that makes an unknown/order call impossible.
    let allowed: &[(&str, &str)] = &[
        ("POST", "/oauth2/token"),
        ("GET", "/n2/acctinfo"),
        ("POST", "/krstock/inquiry/v1/balance"),
        ("POST", "/krstock/quote/v1/currentPrice"),
    ];
    for call in &recorded {
        let path = call.target.split('?').next().unwrap_or(&call.target);
        assert!(
            allowed.contains(&(call.method.as_str(), path)),
            "unexpected call {} {}",
            call.method,
            call.target
        );
    }

    // No order/buy/sell/cancel/reserve path segment anywhere..
    for call in &recorded {
        let path = call.target.split('?').next().unwrap_or(&call.target);
        for forbidden in ["order", "buy", "sell", "cancel", "reserve"] {
            assert!(
                !path.to_lowercase().contains(forbidden),
                "forbidden segment `{forbidden}` in {path}"
            );
        }
    }

    // Redaction: the app secret appears only in the official token call
    // (the documented OAuth query, never on data calls); header values are
    // never recorded at all.
    for call in &recorded {
        if call.target.starts_with("/oauth2/token") {
            continue;
        }
        assert!(
            !call.target.contains("fixture-app-secret"),
            "app secret leaked into a non-token call: {}",
            call.target
        );
    }
}

#[tokio::test]
async fn http_401_maps_to_unauthorized() {
    let server = MockNamuServer::start(HashMap::from([
        ("/oauth2/token".to_string(), (200, fixture("token.json"))),
        (
            "/krstock/inquiry/v1/balance".to_string(),
            (
                401,
                r#"{"rsp_cd":"IGW40044","rsp_msg":"expired token"}"#.to_string(),
            ),
        ),
    ]))
    .await;
    let client = client_for(&server).await;

    let error = client.list_holdings("12345678").await.unwrap_err();
    assert_eq!(error, NamuReadError::Unauthorized);
}

#[tokio::test]
async fn transport_error_contains_status_only() {
    let server = MockNamuServer::start(HashMap::from([
        ("/oauth2/token".to_string(), (200, fixture("token.json"))),
        (
            "/krstock/quote/v1/currentPrice".to_string(),
            (503, "gateway is busy".to_string()),
        ),
    ]))
    .await;
    let client = client_for(&server).await;

    let error = client.etf_quote("360750").await.unwrap_err();
    assert!(matches!(error, NamuReadError::Transport(ref status) if status == "503"));
    assert!(
        !format!("{error}").contains("gateway"),
        "transport error must not include the response body"
    );
}

#[test]
fn non_loopback_http_is_rejected() {
    let error = NamuHttpClient::new(
        "http://api.nhplug.com:8443".parse().unwrap(),
        test_credentials(),
    )
    .unwrap_err();
    assert!(matches!(error, NamuReadError::Transport(_)));
}
