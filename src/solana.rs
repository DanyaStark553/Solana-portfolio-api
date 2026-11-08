use crate::{error::AppError, models::Position};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait SolanaProvider: Send + Sync {
    fn fetch_positions<'a>(&'a self, wallet: &'a str) -> BoxFuture<'a, Result<Vec<Position>, AppError>>;
}

impl<T: ?Sized + SolanaProvider> SolanaProvider for Arc<T> {
    fn fetch_positions<'a>(&'a self, wallet: &'a str) -> BoxFuture<'a, Result<Vec<Position>, AppError>> {
        (**self).fetch_positions(wallet)
    }
}

#[derive(Clone, Default)]
pub struct SolanaClient;

impl SolanaClient {
    pub fn new() -> Self {
        Self
    }
}

impl SolanaProvider for SolanaClient {
    fn fetch_positions<'a>(&'a self, wallet: &'a str) -> BoxFuture<'a, Result<Vec<Position>, AppError>> {
        let wallet = wallet.to_string();
        Box::pin(async move {
            if wallet.trim().is_empty() {
                return Err(AppError::InvalidWallet("wallet is required".into()));
            }
            // Deterministic mock fixtures for local development, offline benchmarking, and integration tests.
            Ok(vec![
                Position {
                    token: "SOL".into(),
                    amount: 2.5,
                    usd_price: 150.0,
                },
                Position {
                    token: "USDC".into(),
                    amount: 100.0,
                    usd_price: 1.0,
                },
            ])
        })
    }
}
