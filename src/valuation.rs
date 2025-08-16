use crate::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationResult {
    pub instrument_id: String,
    pub value: f64,
    pub currency: String,
    pub timestamp: DateTime<Utc>,
    pub confidence: f64,
    pub greeks: Option<Greeks>,
    pub risk_metrics: Option<RiskMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Greeks {
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    pub theta: Option<f64>,
    pub vega: Option<f64>,
    pub rho: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub var_1d: Option<f64>,
    pub var_10d: Option<f64>,
    pub expected_shortfall: Option<f64>,
    pub volatility: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketContext {
    pub risk_free_rate: f64,
    pub dividend_yield: Option<f64>,
    pub volatility: Option<f64>,
    pub spot_price: Option<f64>,
    pub forward_curve: Option<HashMap<String, f64>>,
    pub yield_curve: Option<HashMap<String, f64>>,
    pub timestamp: DateTime<Utc>,
}

pub trait Valuator: Send + Sync {
    fn value(&self, instrument: &dyn Instrument, context: &MarketContext) -> Result<ValuationResult>;
    fn calculate_greeks(&self, instrument: &dyn Instrument, context: &MarketContext) -> Result<Greeks>;
    fn calculate_risk_metrics(&self, instrument: &dyn Instrument, context: &MarketContext) -> Result<RiskMetrics>;
}

pub trait Instrument: std::any::Any {
    fn id(&self) -> &str;
    fn instrument_type(&self) -> InstrumentType;
    fn currency(&self) -> &str;
    fn maturity(&self) -> std::option::Option<DateTime<Utc>>;
    fn notional(&self) -> f64;
    
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InstrumentType {
    Stock,
    Bond,
    Option,
    Future,
    Swap,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationRequest {
    pub instrument_ids: Vec<String>,
    pub valuation_date: DateTime<Utc>,
    pub market_context: MarketContext,
    pub include_greeks: bool,
    pub include_risk_metrics: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationResponse {
    pub results: Vec<ValuationResult>,
    pub timestamp: DateTime<Utc>,
    pub total_value: f64,
    pub currency: String,
}
