use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub token: String,
    pub amount: f64,
    pub usd_price: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Portfolio {
    pub wallet: String,
    pub positions: Vec<Position>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PortfolioResponse {
    pub wallet: String,
    pub positions: Vec<Position>,
    pub total_usd: f64,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorResponse {
    pub error: String,
}
