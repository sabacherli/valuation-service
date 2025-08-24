use crate::{MarketContext, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
// use tokio::time::{sleep, Duration};
use std::future::Future;
use std::pin::Pin;

// Submodules for concrete providers
pub mod mock;
pub use mock::*;

// Unified Finnhub namespace (REST + WS)
pub mod finnhub;
pub use finnhub::*;

// High-level abstraction first: object-safe MarketDataProvider trait
pub trait MarketDataProvider: Send + Sync {
    fn get_spot_price<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>>;
    fn get_volatility<'a>(&'a self, symbol: &'a str, expiry: Option<DateTime<Utc>>) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>>;
    fn get_yield_curve<'a>(&'a self, currency: &'a str) -> Pin<Box<dyn Future<Output = Result<HashMap<String, f64>>> + Send + 'a>>;
    fn get_dividend_yield<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>>;
    fn get_market_context<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<MarketContext>> + Send + 'a>>;
}

// Factory to construct concrete providers as trait objects
pub enum MarketDataProviderKind {
    Mock,
    Finnhub,
}

use std::sync::Arc;
pub fn make_market_data_provider(kind: MarketDataProviderKind, api_key: Option<String>) -> Arc<dyn MarketDataProvider> {
    match kind {
        MarketDataProviderKind::Mock => Arc::new(mock::MockMarketDataProvider::new()),
        MarketDataProviderKind::Finnhub => Arc::new(finnhub::FinnhubMarketDataProvider::new(api_key.unwrap_or_default())),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataPoint {
    pub symbol: String,
    pub price: f64,
    pub volume: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

// (Finnhub provider moved near the bottom of the file)

// (Factory moved to the top with the trait)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YieldCurvePoint {
    pub tenor: String,
    pub rate: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilitySurface {
    pub underlying: String,
    pub strikes: Vec<f64>,
    pub expiries: Vec<String>,
    pub volatilities: Vec<Vec<f64>>,
    pub timestamp: DateTime<Utc>,
}

// (Implementations moved to submodules mock.rs and finnhub_rest.rs)
