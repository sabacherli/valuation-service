use crate::{MarketContext, Result, ValuationError};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataPoint {
    pub symbol: String,
    pub price: f64,
    pub volume: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

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

pub trait MarketDataProvider: Send + Sync {
    fn get_spot_price(&self, symbol: &str) -> impl std::future::Future<Output = Result<f64>> + Send;
    fn get_volatility(&self, symbol: &str, expiry: Option<DateTime<Utc>>) -> impl std::future::Future<Output = Result<f64>> + Send;
    fn get_yield_curve(&self, currency: &str) -> impl std::future::Future<Output = Result<HashMap<String, f64>>> + Send;
    fn get_dividend_yield(&self, symbol: &str) -> impl std::future::Future<Output = Result<f64>> + Send;
    fn get_market_context(&self, symbol: &str) -> impl std::future::Future<Output = Result<MarketContext>> + Send;
}

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
    fn get_spot_price(&self, symbol: &str) -> impl std::future::Future<Output = Result<f64>> + Send {
        let result = self.data.get(symbol)
            .map(|d| d.price)
            .unwrap_or(100.0);
        async move { Ok(result) }
    }

    fn get_volatility(&self, symbol: &str, _expiry: Option<DateTime<Utc>>) -> impl std::future::Future<Output = Result<f64>> + Send {
        let result = self.volatilities.get(symbol).copied().unwrap_or(0.25);
        async move { Ok(result) }
    }

    fn get_yield_curve(&self, currency: &str) -> impl std::future::Future<Output = Result<HashMap<String, f64>>> + Send {
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
        async move { Ok(result) }
    }

    fn get_dividend_yield(&self, symbol: &str) -> impl std::future::Future<Output = Result<f64>> + Send {
        let result = self.dividend_yields.get(symbol).copied().unwrap_or(0.015);
        async move { Ok(result) }
    }

    fn get_market_context(&self, symbol: &str) -> impl std::future::Future<Output = Result<MarketContext>> + Send {
        let spot_price = self.data.get(symbol)
            .map(|d| d.price)
            .unwrap_or(100.0);
        let volatility = self.volatilities.get(symbol).copied().unwrap_or(0.25);
        let dividend_yield = self.dividend_yields.get(symbol).copied().unwrap_or(0.015);
        
        // Get risk-free rate from yield curve (using 1Y rate)
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

        async move {
            Ok(MarketContext {
                risk_free_rate,
                dividend_yield: Some(dividend_yield),
                volatility: Some(volatility),
                spot_price: Some(spot_price),
                forward_curve: None,
                yield_curve: Some(yield_curve),
                timestamp: Utc::now(),
            })
        }
    }
}

pub struct LiveMarketDataProvider {
    client: Client,
    api_key: Option<String>,
    base_url: String,
}

impl LiveMarketDataProvider {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.example.com".to_string(), // Replace with actual API
        }
    }

    async fn fetch_with_retry<T>(&self, url: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 3;
        const RETRY_DELAY: Duration = Duration::from_millis(1000);

        loop {
            attempts += 1;
            
            let mut request = self.client.get(url);
            if let Some(ref api_key) = self.api_key {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<T>().await {
                            Ok(data) => return Ok(data),
                            Err(e) => {
                                if attempts >= MAX_ATTEMPTS {
                                    return Err(ValuationError::Network(e));
                                }
                            }
                        }
                    } else if attempts >= MAX_ATTEMPTS {
                        return Err(ValuationError::MarketData(
                            format!("HTTP error: {}", response.status())
                        ));
                    }
                }
                Err(e) => {
                    if attempts >= MAX_ATTEMPTS {
                        return Err(ValuationError::Network(e));
                    }
                }
            }

            sleep(RETRY_DELAY).await;
        }
    }
}

impl MarketDataProvider for LiveMarketDataProvider {
    async fn get_spot_price(&self, symbol: &str) -> Result<f64> {
        let url = format!("{}/quote/{}", self.base_url, symbol);
        let data: MarketDataPoint = self.fetch_with_retry(&url).await?;
        Ok(data.price)
    }

    async fn get_volatility(&self, symbol: &str, expiry: Option<DateTime<Utc>>) -> Result<f64> {
        let url = match expiry {
            Some(exp) => format!("{}/volatility/{}?expiry={}", self.base_url, symbol, exp.format("%Y-%m-%d")),
            None => format!("{}/volatility/{}", self.base_url, symbol),
        };
        
        #[derive(Deserialize)]
        struct VolatilityResponse {
            volatility: f64,
        }
        
        let data: VolatilityResponse = self.fetch_with_retry(&url).await?;
        Ok(data.volatility)
    }

    async fn get_yield_curve(&self, currency: &str) -> Result<HashMap<String, f64>> {
        let url = format!("{}/yield-curve/{}", self.base_url, currency);
        let data: HashMap<String, f64> = self.fetch_with_retry(&url).await?;
        Ok(data)
    }

    async fn get_dividend_yield(&self, symbol: &str) -> Result<f64> {
        let url = format!("{}/dividend/{}", self.base_url, symbol);
        
        #[derive(Deserialize)]
        struct DividendResponse {
            yield_rate: f64,
        }
        
        let data: DividendResponse = self.fetch_with_retry(&url).await?;
        Ok(data.yield_rate)
    }

    async fn get_market_context(&self, symbol: &str) -> Result<MarketContext> {
        let spot_price = self.get_spot_price(symbol).await?;
        let volatility = self.get_volatility(symbol, None).await?;
        let dividend_yield = self.get_dividend_yield(symbol).await?;
        let yield_curve = self.get_yield_curve("USD").await?;
        
        let risk_free_rate = yield_curve.get("1Y").copied().unwrap_or(0.045);

        Ok(MarketContext {
            risk_free_rate,
            dividend_yield: Some(dividend_yield),
            volatility: Some(volatility),
            spot_price: Some(spot_price),
            forward_curve: None,
            yield_curve: Some(yield_curve),
            timestamp: Utc::now(),
        })
    }
}
