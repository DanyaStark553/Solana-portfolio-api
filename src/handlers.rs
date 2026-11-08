use crate::{
    error::AppError,
    models::PortfolioResponse,
    service::PortfolioService,
};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct PortfolioQuery {
    pub wallet: Option<String>,
}

pub fn router(service: Arc<PortfolioService>) -> Router {
    Router::new()
        .route("/portfolio", get(get_portfolio))
        .with_state(service)
}

async fn get_portfolio(
    State(service): State<Arc<PortfolioService>>,
    query: Result<Query<PortfolioQuery>, QueryRejection>,
) -> Result<Json<PortfolioResponse>, AppError> {
    let Query(query) = query.map_err(|e| {
        AppError::InvalidWallet(format!("invalid query parameters: {e}"))
    })?;

    let wallet = query
        .wallet
        .ok_or_else(|| AppError::InvalidWallet("wallet parameter is required".into()))?;

    let response = service.get_portfolio(&wallet).await?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Position;
    use crate::repository::MemoryRepository;
    use crate::solana::{BoxFuture, SolanaClient, SolanaProvider};
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn send_raw_http_request(addr: SocketAddr, request_str: &str) -> (u16, String, String) {
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to test server");
        stream
            .write_all(request_str.as_bytes())
            .await
            .expect("write request to stream");

        let mut buffer = Vec::new();
        stream
            .read_to_end(&mut buffer)
            .await
            .expect("read response from stream");

        let response = String::from_utf8_lossy(&buffer).to_string();
        let status_line = response.lines().next().unwrap_or("");
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);

        let parts: Vec<&str> = response.split("\r\n\r\n").collect();
        let headers = parts.first().copied().unwrap_or("").to_string();
        let body = parts.get(1).copied().unwrap_or("").to_string();

        (status_code, headers, body)
    }

    struct SlowMockSolanaProvider;

    impl SolanaProvider for SlowMockSolanaProvider {
        fn fetch_positions<'a>(&'a self, _wallet: &'a str) -> BoxFuture<'a, Result<Vec<Position>, AppError>> {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(vec![Position {
                    token: "SOL".into(),
                    amount: 1.0,
                    usd_price: 100.0,
                }])
            })
        }
    }

    struct FailingMockSolanaProvider;

    impl SolanaProvider for FailingMockSolanaProvider {
        fn fetch_positions<'a>(&'a self, _wallet: &'a str) -> BoxFuture<'a, Result<Vec<Position>, AppError>> {
            Box::pin(async move {
                Err(AppError::UpstreamError("RPC node unreachable".into()))
            })
        }
    }

    #[tokio::test]
    async fn test_handler_successful_portfolio_request() {
        let repo = Arc::new(MemoryRepository::new());
        let service = Arc::new(PortfolioService::new(repo, SolanaClient::new()));
        let app = router(service);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let req = format!(
            "GET /portfolio?wallet=test_wallet HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            addr
        );
        let (status, headers, body) = send_raw_http_request(addr, &req).await;

        assert_eq!(status, 200);
        assert!(headers.contains("content-type: application/json"));
        assert!(body.contains("\"wallet\":\"test_wallet\""));
        assert!(body.contains("\"total_usd\":475.0"));
    }

    #[tokio::test]
    async fn test_handler_missing_wallet_returns_400_json() {
        let repo = Arc::new(MemoryRepository::new());
        let service = Arc::new(PortfolioService::new(repo, SolanaClient::new()));
        let app = router(service);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Test GET /portfolio (missing wallet query parameter entirely)
        let req = format!(
            "GET /portfolio HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            addr
        );
        let (status, headers, body) = send_raw_http_request(addr, &req).await;

        assert_eq!(status, 400);
        assert!(headers.contains("content-type: application/json"));
        assert!(body.contains("\"error\":\"wallet parameter is required\""));
    }

    #[tokio::test]
    async fn test_handler_blank_wallet_returns_400_json() {
        let repo = Arc::new(MemoryRepository::new());
        let service = Arc::new(PortfolioService::new(repo, SolanaClient::new()));
        let app = router(service);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Test GET /portfolio?wallet=   (blank/whitespace wallet)
        let req = format!(
            "GET /portfolio?wallet=%20%20%20 HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            addr
        );
        let (status, headers, body) = send_raw_http_request(addr, &req).await;

        assert_eq!(status, 400);
        assert!(headers.contains("content-type: application/json"));
        assert!(body.contains("\"error\":\"wallet parameter cannot be empty\""));
    }

    #[tokio::test]
    async fn test_handler_upstream_timeout_returns_504_json() {
        let repo = Arc::new(MemoryRepository::new());
        let slow_provider = SlowMockSolanaProvider;
        let service = Arc::new(
            PortfolioService::new(repo, slow_provider)
                .with_timeout(Duration::from_millis(50)), // 50ms timeout < 200ms sleep
        );
        let app = router(service);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let req = format!(
            "GET /portfolio?wallet=test_wallet HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            addr
        );
        let (status, headers, body) = send_raw_http_request(addr, &req).await;

        assert_eq!(status, 504);
        assert!(headers.contains("content-type: application/json"));
        assert!(body.contains("\"error\":\"Upstream service timed out\""));
    }

    #[tokio::test]
    async fn test_handler_upstream_error_returns_502_json() {
        let repo = Arc::new(MemoryRepository::new());
        let failing_provider = FailingMockSolanaProvider;
        let service = Arc::new(PortfolioService::new(repo, failing_provider));
        let app = router(service);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let req = format!(
            "GET /portfolio?wallet=test_wallet HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            addr
        );
        let (status, headers, body) = send_raw_http_request(addr, &req).await;

        assert_eq!(status, 502);
        assert!(headers.contains("content-type: application/json"));
        assert!(body.contains("\"error\":\"Upstream service failed: RPC node unreachable\""));
    }
}
