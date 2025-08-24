//! Finnhub market data providers (REST and WebSocket)
//!
//! This module contains two cohesive parts:
//! - REST provider: `FinnhubMarketDataProvider` implements `MarketDataProvider` for on-demand data
//! - WebSocket provider: `LiveFinnhubWs` implements `PriceStreamProvider` for live streaming
//!
//! Rationale:
//! - REST is request/response and integrates via the object-safe `MarketDataProvider` trait.
//! - WS is a long-lived loop with reconnect/backoff, and writes prices to the DB.
//! - Keeping both in one file improves discoverability while keeping sections clearly split.
//!
//! Quick usage:
//! - REST: create via `FinnhubMarketDataProvider::new(api_key)` and call trait methods via the factory in `market_data::make_market_data_provider()`.
//! - WS: construct `LiveFinnhubWs` and call `spawn(state)` or `run_finnhub_ws(api_key, state)`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use futures_util::{SinkExt, StreamExt};

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use sqlx::{Pool, Postgres, Row};
use tokio::time::{sleep, Duration};
use tokio_tungstenite::connect_async;
use tracing::{error, info, warn};
use url::Url;

use crate::ValuationError;
use super::{MarketContext, MarketDataProvider, Result};

// ===================== REST provider =====================
pub struct FinnhubMarketDataProvider {
    client: Client,
    api_key: String,
}

impl FinnhubMarketDataProvider {
    pub fn new<S: Into<String>>(api_key: S) -> Self {
        Self { client: Client::new(), api_key: api_key.into() }
    }
}

impl FinnhubMarketDataProvider {
    async fn do_get_spot_price(&self, symbol: &str) -> Result<f64> {
        #[derive(Deserialize)]
        struct Quote { #[serde(default)] c: f64 }
        let url = format!("https://finnhub.io/api/v1/quote?symbol={}&token={}", symbol, self.api_key);
        let resp = self.client.get(&url).send().await.map_err(ValuationError::Network)?;
        if !resp.status().is_success() {
            return Err(ValuationError::MarketData(format!("HTTP {}", resp.status())));
        }
        let q: Quote = resp.json().await.map_err(ValuationError::Network)?;
        Ok(q.c)
    }

    async fn do_get_volatility(&self, _symbol: &str, _expiry: Option<DateTime<Utc>>) -> Result<f64> {
        Ok(0.25)
    }

    async fn do_get_yield_curve(&self, _currency: &str) -> Result<HashMap<String, f64>> {
        let mut curve = HashMap::new();
        curve.insert("1M".to_string(), 0.045);
        curve.insert("3M".to_string(), 0.047);
        curve.insert("6M".to_string(), 0.048);
        curve.insert("1Y".to_string(), 0.0485);
        curve.insert("2Y".to_string(), 0.049);
        curve.insert("5Y".to_string(), 0.051);
        curve.insert("10Y".to_string(), 0.053);
        Ok(curve)
    }

    async fn do_get_dividend_yield(&self, _symbol: &str) -> Result<f64> { Ok(0.0) }

    async fn do_get_market_context(&self, symbol: &str) -> Result<MarketContext> {
        let spot_price = self.do_get_spot_price(symbol).await?;
        let volatility = self.do_get_volatility(symbol, None).await?;
        let yield_curve = self.do_get_yield_curve("USD").await?;
        let risk_free_rate = yield_curve.get("1Y").copied().unwrap_or(0.045);
        let dividend_yield = self.do_get_dividend_yield(symbol).await?;

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

impl MarketDataProvider for FinnhubMarketDataProvider {
    fn get_spot_price<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>> {
        Box::pin(self.do_get_spot_price(symbol))
    }
    fn get_volatility<'a>(&'a self, symbol: &'a str, expiry: Option<DateTime<Utc>>) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>> {
        Box::pin(self.do_get_volatility(symbol, expiry))
    }
    fn get_yield_curve<'a>(&'a self, currency: &'a str) -> Pin<Box<dyn Future<Output = Result<HashMap<String, f64>>> + Send + 'a>> {
        Box::pin(self.do_get_yield_curve(currency))
    }
    fn get_dividend_yield<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<f64>> + Send + 'a>> {
        Box::pin(self.do_get_dividend_yield(symbol))
    }
    fn get_market_context<'a>(&'a self, symbol: &'a str) -> Pin<Box<dyn Future<Output = Result<MarketContext>> + Send + 'a>> {
        Box::pin(self.do_get_market_context(symbol))
    }
}

// ===================== WebSocket provider =====================

// Minimal local AppState to avoid depending on the binary crate module tree
#[derive(Clone)]
pub struct AppState {
    pub db: Pool<Postgres>,
}

#[derive(Debug, Deserialize)]
struct FinnhubTrade { p: f64, s: String }

#[derive(Debug, Deserialize)]
struct FinnhubMsg {
    #[serde(default)] r#type: String,
    #[serde(default)] data: Vec<FinnhubTrade>,
}

// Generic streaming trait
pub trait PriceStreamProvider: Send + Sync + 'static {
    fn spawn(self: Arc<Self>, state: Arc<AppState>) -> tokio::task::JoinHandle<()>;
}

pub struct LiveFinnhubWs { api_key: String }

impl LiveFinnhubWs {
    pub fn new(api_key: String) -> Self { Self { api_key } }

    async fn run_loop(self: Arc<Self>, state: Arc<AppState>) {
        let mut backoff_secs = 1u64;
        loop {
            let url = Url::parse(&format!("wss://ws.finnhub.io?token={}", self.api_key)).expect("valid url");
            info!("Connecting to Finnhub WebSocket: {}", url);
            match connect_async(url).await {
                Ok((mut ws, _resp)) => {
                    info!("Connected to Finnhub WebSocket");
                    backoff_secs = 1;

                    let syms = current_instrument_symbols(&state.db).await;
                    for s in syms.iter() {
                        let sub = format!("{{\"type\":\"subscribe\",\"symbol\":\"{}\"}}", s);
                        if let Err(e) = ws.send(tokio_tungstenite::tungstenite::Message::Text(sub)).await {
                            warn!("Failed to send subscribe for {}: {}", s, e);
                        }
                    }

                    while let Some(msg) = ws.next().await {
                        match msg {
                            Ok(tokio_tungstenite::tungstenite::Message::Text(txt)) => {
                                if let Ok(parsed) = serde_json::from_str::<FinnhubMsg>(&txt) {
                                    if parsed.r#type == "trade" {
                                        for t in parsed.data {
                                            let _ = sqlx::query("INSERT INTO instruments (symbol, price) VALUES ($1, $2) ON CONFLICT (symbol) DO UPDATE SET price = EXCLUDED.price")
                                                .bind(&t.s)
                                                .bind(t.p)
                                                .execute(&state.db)
                                                .await;

                                            let _ = sqlx::query("INSERT INTO price_history (symbol, price, ts) VALUES ($1, $2, NOW())")
                                                .bind(&t.s)
                                                .bind(t.p)
                                                .execute(&state.db)
                                                .await;
                                        }
                                    }
                                }
                            }
                            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => { warn!("Finnhub WS closed by server"); break; }
                            Ok(_) => {}
                            Err(e) => { warn!("Finnhub WS recv error: {}", e); break; }
                        }
                    }
                    warn!("Disconnected from Finnhub WS, will reconnect");
                }
                Err(e) => { error!("Failed to connect to Finnhub WS: {}", e); }
            }
            sleep(Duration::from_secs(backoff_secs.min(30))).await;
            backoff_secs = (backoff_secs * 2).min(30);
        }
    }
}

impl PriceStreamProvider for LiveFinnhubWs {
    fn spawn(self: Arc<Self>, state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move { self.run_loop(state).await })
    }
}

async fn current_instrument_symbols(db: &Pool<Postgres>) -> Vec<String> {
    match sqlx::query("SELECT symbol FROM instruments ORDER BY symbol ASC").fetch_all(db).await {
        Ok(rows) => rows.into_iter().filter_map(|r| r.try_get::<String, _>("symbol").ok()).collect(),
        Err(_) => Vec::new(),
    }
}

pub async fn run_finnhub_ws(api_key: String, state: Arc<AppState>) {
    let provider = Arc::new(LiveFinnhubWs::new(api_key));
    provider.run_loop(state).await;
}
