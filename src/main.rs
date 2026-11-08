use solana_portfolio_api::{
    config::Config,
    handlers::router,
    repository::MemoryRepository,
    service::PortfolioService,
    solana::SolanaClient,
};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let config = Config::load();
    let repo = Arc::new(MemoryRepository::new());
    let service = Arc::new(
        PortfolioService::new(repo, SolanaClient::new())
            .with_timeout(Duration::from_millis(config.request_timeout_ms)),
    );
    let app = router(service);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port))
        .await
        .expect("bind listener");
    println!("API listening on :{}", config.port);
    axum::serve(listener, app).await.expect("serve");
}
