use crate::{error::AppError, models::Portfolio};
use std::{collections::HashMap, sync::RwLock};

pub trait Repository: Send + Sync {
    fn get(&self, wallet: &str) -> Result<Option<Portfolio>, AppError>;
    fn save(&self, portfolio: Portfolio) -> Result<(), AppError>;
}

pub struct MemoryRepository {
    data: RwLock<HashMap<String, Portfolio>>,
}

impl MemoryRepository {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl Repository for MemoryRepository {
    fn get(&self, wallet: &str) -> Result<Option<Portfolio>, AppError> {
        let guard = self
            .data
            .read()
            .map_err(|_| AppError::Storage("repository read lock poisoned".into()))?;
        Ok(guard.get(wallet).cloned())
    }

    fn save(&self, portfolio: Portfolio) -> Result<(), AppError> {
        let mut guard = self
            .data
            .write()
            .map_err(|_| AppError::Storage("repository write lock poisoned".into()))?;
        guard.insert(portfolio.wallet.clone(), portfolio);
        Ok(())
    }
}
