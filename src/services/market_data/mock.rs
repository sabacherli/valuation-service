use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use chrono::{DateTime, Utc};

use super::{MarketContext, MarketDataPoint, MarketDataProvider, Result};

pub struct MockMarketDataProvider {
    pub data: HashMap<String, MarketDataPoint>,
    pub volatilities: HashMap<String, f64>,
    pub yield_curves: HashMap<String, HashMap<String, f64>>,
    pub dividend_yields: HashMap<String, f64>,
}

impl MockMarketDataProvider {
    pub fn new() -> Self {
        let mut data = HashMap::new();
        let mut volatilities = HashMap::new();
        let mut yield_curves = HashMap::new();
        let mut dividend_yields = HashMap::new();

        // Sample market data
        data.insert("AAPL".to_string(), MarketDataPoint {
            symbol: "AAPL".to_string(),
            price: 175.50,
            volume: Some(50_000_000.0),
            bid: Some(175.49),
            ask: Some(175.51),
            timestamp: Utc::now(),
        });

        data.insert("MSFT".to_string(), MarketDataPoint {
            symbol: "MSFT".to_string(),
            price: 415.25,
            volume: Some(25_000_000.0),
            bid: Some(415.24),
            ask: Some(415.26),
            timestamp: Utc::now(),
        });

        data.insert("GOOGL".to_string(), MarketDataPoint {
            symbol: "GOOGL".to_string(),
            price: 142.80,
            volume: Some(30_000_000.0),
            bid: Some(142.79),
            ask: Some(142.81),
            timestamp: Utc::now(),
        });

        // Sample volatilities (annualized)
        volatilities.insert("AAPL".to_string(), 0.25);
        volatilities.insert("MSFT".to_string(), 0.22);
        volatilities.insert("GOOGL".to_string(), 0.28);

        // Sample yield curve for USD
        let mut usd_curve = HashMap::new();
        usd_curve.insert("1M".to_string(), 0.0525);
        usd_curve.insert("3M".to_string(), 0.0535);
        usd_curve.insert("6M".to_string(), 0.0545);
        usd_curve.insert("1Y".to_string(), 0.0485);
        usd_curve.insert("2Y".to_string(), 0.0445);
        usd_curve.insert("5Y".to_string(), 0.0425);
        usd_curve.insert("10Y".to_string(), 0.0435);
        usd_curve.insert("30Y".to_string(), 0.0445);
        yield_curves.insert("USD".to_string(), usd_curve);

        // Sample dividend yields
        dividend_yields.insert("AAPL".to_string(), 0.0045);
        dividend_yields.insert("MSFT".to_string(), 0.0068);
        dividend_yields.insert("GOOGL".to_string(), 0.0000);

        Self {
            data,
            volatilities,
            yield_curves,
            dividend_yields,
        }
    }

    pub fn update_price(&mut self, symbol: &str, price: f64) {
        if let Some(data_point) = self.data.get_mut(symbol) {
            data_point.price = price;
            data_point.timestamp = Utc::now();
        }
    }

    pub fn add_instrument(&mut self, symbol: String, price: f64, volatility: f64, dividend_yield: f64) {
        self.data.insert(symbol.clone(), MarketDataPoint {
            symbol: symbol.clone(),
            price,
            volume: Some(1_000_000.0),
            bid: Some(price - 0.01),
            ask: Some(price + 0.01),
            timestamp: Utc::now(),
        });
        self.volatilities.insert(symbol.clone(), volatility);
        self.dividend_yields.insert(symbol, dividend_yield);
    }
}

impl MarketDataProvider for MockMarketDataProvider {
    fn get_spot_price<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>> {
        let result = self.data.get(symbol).map(|d| d.price).unwrap_or(100.0);
        Box::pin(async move { Ok(result) })
    }

    fn get_volatility<'a>(&'a self, symbol: &'a str, _expiry: Option<DateTime<Utc>>) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>> {
        let result = self.volatilities.get(symbol).copied().unwrap_or(0.25);
        Box::pin(async move { Ok(result) })
    }

    fn get_yield_curve<'a>(&'a self, currency: &'a str) -> Pin<Box<dyn Future<Output = Result<HashMap<String, f64>>> + Send + 'a>> {
        let result = self.yield_curves.get(currency).cloned().unwrap_or_else(|| {
            let mut curve = HashMap::new();
            curve.insert("1M".to_string(), 0.045);
            curve.insert("3M".to_string(), 0.047);
            curve.insert("6M".to_string(), 0.048);
            curve.insert("1Y".to_string(), 0.0485);
            curve.insert("2Y".to_string(), 0.049);
            curve.insert("5Y".to_string(), 0.051);
            curve.insert("10Y".to_string(), 0.053);
            curve
        });
        Box::pin(async move { Ok(result) })
    }

    fn get_dividend_yield<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>> {
        let result = self.dividend_yields.get(symbol).copied().unwrap_or(0.015);
        Box::pin(async move { Ok(result) })
    }

    fn get_market_context<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<MarketContext>> + Send + 'a>> {
        let spot_price = self.data.get(symbol).map(|d| d.price).unwrap_or(100.0);
        let volatility = self.volatilities.get(symbol).copied().unwrap_or(0.25);
        let dividend_yield = self.dividend_yields.get(symbol).copied().unwrap_or(0.015);
        let yield_curve = self.yield_curves.get("USD").cloned().unwrap_or_else(|| {
            let mut curve = HashMap::new();
            curve.insert("1M".to_string(), 0.045);
            curve.insert("3M".to_string(), 0.047);
            curve.insert("6M".to_string(), 0.048);
            curve.insert("1Y".to_string(), 0.0485);
            curve.insert("2Y".to_string(), 0.049);
            curve.insert("5Y".to_string(), 0.051);
            curve.insert("10Y".to_string(), 0.053);
            curve
        });
        let risk_free_rate = yield_curve.get("1Y").copied().unwrap_or(0.0485);
        Box::pin(async move {
            Ok(MarketContext {
                risk_free_rate,
                dividend_yield: Some(dividend_yield),
                volatility: Some(volatility),
                spot_price: Some(spot_price),
                forward_curve: None,
                yield_curve: Some(yield_curve),
                timestamp: Utc::now(),
            })
        })
    }
}
