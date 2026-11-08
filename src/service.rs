use crate::{
    error::AppError,
    models::{Portfolio, PortfolioResponse, Position},
    repository::Repository,
    solana::SolanaProvider,
};
use std::sync::Arc;
use std::time::Duration;

pub fn normalize_wallet(wallet: &str) -> Result<String, AppError> {
    let trimmed = wallet.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidWallet("wallet parameter cannot be empty".into()));
    }
    // Reject control characters or interior whitespace
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AppError::InvalidWallet(
            "wallet parameter contains invalid whitespace or control characters".into(),
        ));
    }
    // Practical length constraint (Solana Base58 public keys are 32-44 chars; allow up to 128 characters for local fixtures or sub-accounts)
    if trimmed.len() > 128 {
        return Err(AppError::InvalidWallet(
            "wallet parameter exceeds maximum allowable length of 128 characters".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Computes the total USD value of positions with strict float validation.
///
/// In financial/DeFi applications, silent fallback or ignoring bad feeds leads to incorrect balances.
/// Thus, this function strictly rejects:
/// - Empty token symbols
/// - Negative amounts or negative prices
/// - Non-finite numbers (`NaN`, `+Infinity`, `-Infinity`)
/// - Float overflow during multiplication or total sum
pub fn calculate_total_usd(positions: &[Position]) -> Result<f64, AppError> {
    let mut total_usd: f64 = 0.0;

    for (idx, p) in positions.iter().enumerate() {
        if p.token.trim().is_empty() {
            return Err(AppError::InvalidPositionData(format!(
                "position at index {idx} has an empty token symbol"
            )));
        }
        if !p.amount.is_finite() || p.amount < 0.0 {
            return Err(AppError::InvalidPositionData(format!(
                "token '{}' has invalid amount: {}",
                p.token, p.amount
            )));
        }
        if !p.usd_price.is_finite() || p.usd_price < 0.0 {
            return Err(AppError::InvalidPositionData(format!(
                "token '{}' has invalid usd_price: {}",
                p.token, p.usd_price
            )));
        }

        let value = p.amount * p.usd_price;
        if !value.is_finite() || value < 0.0 {
            return Err(AppError::InvalidPositionData(format!(
                "token '{}' valuation overflowed or is non-finite",
                p.token
            )));
        }

        total_usd += value;
        if !total_usd.is_finite() {
            return Err(AppError::InvalidPositionData(
                "portfolio total valuation overflowed or is non-finite".into(),
            ));
        }
    }

    Ok(total_usd)
}

pub struct PortfolioService {
    repo: Arc<dyn Repository>,
    solana: Arc<dyn SolanaProvider>,
    timeout: Duration,
}

impl PortfolioService {
    /// Creates a new `PortfolioService` with a default 5000ms request timeout.
    /// Preserves full signature compatibility with the starter codebase.
    pub fn new<S: SolanaProvider + 'static>(repo: Arc<dyn Repository>, solana: S) -> Self {
        Self {
            repo,
            solana: Arc::new(solana),
            timeout: Duration::from_millis(5000),
        }
    }

    /// Builder pattern method to configure a custom request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn get_portfolio(&self, wallet: &str) -> Result<PortfolioResponse, AppError> {
        let normalized_wallet = normalize_wallet(wallet)?;

        let (portfolio, total_usd) = match self.repo.get(&normalized_wallet)? {
            Some(p) => {
                let total = calculate_total_usd(&p.positions)?;
                (p, total)
            }
            None => {
                let fetch_future = self.solana.fetch_positions(&normalized_wallet);
                let positions = tokio::time::timeout(self.timeout, fetch_future)
                    .await
                    .map_err(|_| AppError::UpstreamTimeout)??;

                // Validate positions before caching to guarantee the repository is never poisoned with invalid data
                let total = calculate_total_usd(&positions)?;

                let p = Portfolio {
                    wallet: normalized_wallet.clone(),
                    positions,
                    updated_at: chrono::Utc::now().to_rfc3339(),
                };
                self.repo.save(p.clone())?;
                (p, total)
            }
        };

        Ok(PortfolioResponse {
            wallet: portfolio.wallet,
            positions: portfolio.positions,
            total_usd,
            updated_at: portfolio.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Position;
    use crate::repository::MemoryRepository;
    use crate::solana::BoxFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_calculate_total_usd_valid() {
        let positions = vec![
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
        ];
        let total = calculate_total_usd(&positions).expect("valid positions calculation");
        assert_eq!(total, 475.0);

        // Empty portfolio is valid and equals 0.0
        let empty_total = calculate_total_usd(&[]).expect("empty positions calculation");
        assert_eq!(empty_total, 0.0);
    }

    #[test]
    fn test_calculate_total_usd_negative_amount_and_price() {
        let neg_amount = vec![Position {
            token: "SOL".into(),
            amount: -1.0,
            usd_price: 150.0,
        }];
        assert!(calculate_total_usd(&neg_amount).is_err());

        let neg_price = vec![Position {
            token: "SOL".into(),
            amount: 1.0,
            usd_price: -10.0,
        }];
        assert!(calculate_total_usd(&neg_price).is_err());
    }

    #[test]
    fn test_calculate_total_usd_nan_and_infinity() {
        let nan_pos = vec![Position {
            token: "SOL".into(),
            amount: f64::NAN,
            usd_price: 150.0,
        }];
        assert!(calculate_total_usd(&nan_pos).is_err());

        let inf_price = vec![Position {
            token: "SOL".into(),
            amount: 2.0,
            usd_price: f64::INFINITY,
        }];
        assert!(calculate_total_usd(&inf_price).is_err());

        let neg_inf_price = vec![Position {
            token: "SOL".into(),
            amount: 2.0,
            usd_price: f64::NEG_INFINITY,
        }];
        assert!(calculate_total_usd(&neg_inf_price).is_err());
    }

    #[test]
    fn test_calculate_total_usd_overflow_and_empty_token() {
        let overflow = vec![Position {
            token: "SOL".into(),
            amount: 1e300,
            usd_price: 1e100,
        }];
        assert!(calculate_total_usd(&overflow).is_err());

        let empty_token = vec![Position {
            token: "   ".into(),
            amount: 10.0,
            usd_price: 1.0,
        }];
        assert!(calculate_total_usd(&empty_token).is_err());
    }

    #[test]
    fn test_normalize_wallet() {
        assert_eq!(
            normalize_wallet("   abc123   ").unwrap(),
            "abc123".to_string()
        );
        assert!(normalize_wallet("").is_err());
        assert!(normalize_wallet("   \t  \n  ").is_err());
        assert!(normalize_wallet("abc 123").is_err()); // Interior whitespace rejected
        assert!(normalize_wallet(&"a".repeat(129)).is_err()); // Exceeds 128 chars
    }

    struct MockSolanaProvider {
        call_count: AtomicUsize,
        response: Result<Vec<Position>, AppError>,
        delay: Option<Duration>,
    }

    impl SolanaProvider for MockSolanaProvider {
        fn fetch_positions<'a>(
            &'a self,
            _wallet: &'a str,
        ) -> BoxFuture<'a, Result<Vec<Position>, AppError>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let resp = self.response.clone();
            let delay = self.delay;
            Box::pin(async move {
                if let Some(d) = delay {
                    tokio::time::sleep(d).await;
                }
                resp
            })
        }
    }

    #[tokio::test]
    async fn test_service_caching_and_miss() {
        let repo = Arc::new(MemoryRepository::new());
        let mock_provider = Arc::new(MockSolanaProvider {
            call_count: AtomicUsize::new(0),
            response: Ok(vec![Position {
                token: "SOL".into(),
                amount: 2.0,
                usd_price: 100.0,
            }]),
            delay: None,
        });

        let service = PortfolioService::new(repo.clone(), mock_provider.clone())
            .with_timeout(Duration::from_millis(500));

        // First call: cache miss, calls provider
        let res1 = service.get_portfolio("  my_wallet  ").await.unwrap();
        assert_eq!(res1.wallet, "my_wallet");
        assert_eq!(res1.total_usd, 200.0);
        assert_eq!(mock_provider.call_count.load(Ordering::SeqCst), 1);

        // Second call: cache hit, provider is not called
        let res2 = service.get_portfolio("my_wallet").await.unwrap();
        assert_eq!(res2.total_usd, 200.0);
        assert_eq!(mock_provider.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_service_invalid_positions_not_cached() {
        let repo = Arc::new(MemoryRepository::new());
        let mock_provider = Arc::new(MockSolanaProvider {
            call_count: AtomicUsize::new(0),
            response: Ok(vec![Position {
                token: "BAD".into(),
                amount: f64::NAN,
                usd_price: 100.0,
            }]),
            delay: None,
        });

        let service = PortfolioService::new(repo.clone(), mock_provider.clone())
            .with_timeout(Duration::from_millis(500));

        let err = service.get_portfolio("wallet_with_nan").await.unwrap_err();
        assert!(matches!(err, AppError::InvalidPositionData(_)));

        // Ensure repo has not cached the failed portfolio
        assert!(repo.get("wallet_with_nan").unwrap().is_none());
    }

    #[tokio::test]
    async fn test_service_upstream_timeout() {
        let repo = Arc::new(MemoryRepository::new());
        let mock_provider = Arc::new(MockSolanaProvider {
            call_count: AtomicUsize::new(0),
            response: Ok(vec![]),
            delay: Some(Duration::from_millis(200)),
        });

        // Service has a 50ms timeout, while mock provider sleeps for 200ms
        let service = PortfolioService::new(repo, mock_provider)
            .with_timeout(Duration::from_millis(50));

        let err = service.get_portfolio("timeout_wallet").await.unwrap_err();
        assert_eq!(err, AppError::UpstreamTimeout);
    }
}
