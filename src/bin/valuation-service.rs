use axum::{
    extract::{Path, State},
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Sse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{Utc, Duration as ChronoDuration};
use futures::stream::Stream;
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, Row};
use std::env;
use std::{collections::HashMap, convert::Infallible, sync::{Arc, Mutex}, time::Duration as StdDuration};
use tokio::sync::broadcast::{self, Sender};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tokio_tungstenite::connect_async;
use tower_http::cors::CorsLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
 

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProviderConfig {
    api_url: String,        // e.g., https://finnhub.io/api/v1
    ws_url: String,         // e.g., wss://ws.finnhub.io
    api_key: String,        // Finnhub API key
    webhook_secret: String, // Secret to protect /price-stream
}

#[derive(Debug, Clone, Serialize, Default)]
struct ProviderConfigPublic {
    api_url: String,
    ws_url: String,
    has_api_key: bool,
    has_webhook_secret: bool,
    api_key_updated_at: Option<String>,
    webhook_secret_updated_at: Option<String>,
}

async fn ensure_provider_config_table(db: &Pool<Postgres>) {
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS provider_config (\n            id INTEGER PRIMARY KEY CHECK (id = 1),\n            api_url TEXT NOT NULL,\n            ws_url TEXT NOT NULL,\n            api_key TEXT NOT NULL,\n            webhook_secret TEXT NOT NULL,\n            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\n        )"
    ).execute(db).await;
    // Add per-secret updated timestamps if missing
    let _ = sqlx::query("ALTER TABLE provider_config ADD COLUMN IF NOT EXISTS api_key_updated_at TIMESTAMPTZ").execute(db).await;
    let _ = sqlx::query("ALTER TABLE provider_config ADD COLUMN IF NOT EXISTS webhook_secret_updated_at TIMESTAMPTZ").execute(db).await;
    // Backfill from updated_at when secrets are present but per-secret timestamps are NULL
    let _ = sqlx::query("UPDATE provider_config SET api_key_updated_at = COALESCE(api_key_updated_at, updated_at) WHERE api_key_updated_at IS NULL AND api_key <> ''").execute(db).await;
    let _ = sqlx::query("UPDATE provider_config SET webhook_secret_updated_at = COALESCE(webhook_secret_updated_at, updated_at) WHERE webhook_secret_updated_at IS NULL AND webhook_secret <> ''").execute(db).await;
}

async fn load_provider_config(db: &Pool<Postgres>) -> ProviderConfig {
    if let Ok(row) = sqlx::query("SELECT api_url, ws_url, api_key, webhook_secret FROM provider_config WHERE id = 1")
        .fetch_one(db)
        .await
    {
        let api_url: String = row.get("api_url");
        let ws_url: String = row.get("ws_url");
        let api_key: String = row.get("api_key");
        let webhook_secret: String = row.get("webhook_secret");
        return ProviderConfig { api_url, ws_url, api_key, webhook_secret };
    }
    // Defaults from env for bootstrap
    let api_key = env::var("FINNHUB_API_KEY").unwrap_or_default();
    let webhook_secret = env::var("WEBHOOK_SECRET").unwrap_or_default();
    let cfg = ProviderConfig {
        api_url: "https://finnhub.io/api/v1".to_string(),
        ws_url: "wss://ws.finnhub.io".to_string(),
        api_key,
        webhook_secret,
    };
    let _ = sqlx::query("INSERT INTO provider_config (id, api_url, ws_url, api_key, webhook_secret) VALUES (1, $1, $2, $3, $4) ON CONFLICT (id) DO UPDATE SET api_url = EXCLUDED.api_url, ws_url = EXCLUDED.ws_url, api_key = EXCLUDED.api_key, webhook_secret = EXCLUDED.webhook_secret, updated_at = NOW()")
        .bind(&cfg.api_url)
        .bind(&cfg.ws_url)
        .bind(&cfg.api_key)
        .bind(&cfg.webhook_secret)
        .execute(db)
        .await;
    cfg
}

// Admin endpoints are always open; no admin-secret enforcement

 

#[derive(Debug, Deserialize)]
struct ProviderConfigUpdate {
    api_key: Option<String>,
    webhook_secret: Option<String>,
}

// GET /admin/provider-config
async fn get_provider_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = state.provider_config.read().await.clone();
    // Read timestamps directly from DB to avoid widening in-memory struct
    let row = sqlx::query("SELECT COALESCE(api_key_updated_at, updated_at) AS api_key_updated_at, COALESCE(webhook_secret_updated_at, updated_at) AS webhook_secret_updated_at FROM provider_config WHERE id = 1")
        .fetch_one(&state.db)
        .await;
    let (api_key_updated_at, webhook_secret_updated_at) = match row {
        Ok(r) => {
            let a: Option<chrono::DateTime<chrono::Utc>> = r.try_get("api_key_updated_at").ok();
            let b: Option<chrono::DateTime<chrono::Utc>> = r.try_get("webhook_secret_updated_at").ok();
            (a.map(|t| t.to_rfc3339()), b.map(|t| t.to_rfc3339()))
        }
        Err(_) => (None, None),
    };
    let public = ProviderConfigPublic {
        api_url: cfg.api_url,
        ws_url: cfg.ws_url,
        has_api_key: !cfg.api_key.is_empty(),
        has_webhook_secret: !cfg.webhook_secret.is_empty(),
        api_key_updated_at,
        webhook_secret_updated_at,
    };
    (StatusCode::OK, Json(public)).into_response()
}

// PUT /admin/provider-config
async fn update_provider_config(State(state): State<Arc<AppState>>, Json(payload): Json<ProviderConfigUpdate>) -> impl IntoResponse {
    let mut cfg = state.provider_config.read().await.clone();
    // Update API key if provided
    if let Some(v) = payload.api_key {
        cfg.api_key = v.trim().to_string();
        let _ = sqlx::query("UPDATE provider_config SET api_key = $1, api_key_updated_at = NOW(), updated_at = NOW() WHERE id = 1")
            .bind(&cfg.api_key)
            .execute(&state.db)
            .await;
    }
    // Update webhook secret if provided
    if let Some(v) = payload.webhook_secret {
        cfg.webhook_secret = v.trim().to_string();
        let _ = sqlx::query("UPDATE provider_config SET webhook_secret = $1, webhook_secret_updated_at = NOW(), updated_at = NOW() WHERE id = 1")
            .bind(&cfg.webhook_secret)
            .execute(&state.db)
            .await;
    }
    {
        let mut w = state.provider_config.write().await;
        *w = cfg.clone();
    }
    (StatusCode::OK, Json(json!({"status":"updated"}))).into_response()
}

async fn current_api_key(state: &AppState) -> String {
    let key = state.provider_config.read().await.api_key.clone();
    if !key.is_empty() { key } else { env::var("FINNHUB_API_KEY").unwrap_or_default() }
}

async fn current_webhook_secret(state: &AppState) -> String {
    let s = state.provider_config.read().await.webhook_secret.clone();
    if !s.is_empty() { s } else { env::var("WEBHOOK_SECRET").unwrap_or_default() }
}
async fn current_api_base(state: &AppState) -> String {
    let base = state.provider_config.read().await.api_url.clone();
    if !base.is_empty() { base } else { "https://finnhub.io/api/v1".to_string() }
}
async fn current_ws_base(state: &AppState) -> String {
    let base = state.provider_config.read().await.ws_url.clone();
    if !base.is_empty() { base } else { "wss://ws.finnhub.io".to_string() }
}
// (Removed old manual instrument upsert types)

// ----- Price stream SSE proxy to Finnhub -----
#[derive(Debug, Deserialize)]
struct PriceStreamQuery { symbols: Option<String>, secret: Option<String> }

#[derive(Debug, Deserialize)]
struct SymbolSearchQuery { q: String, exchange: Option<String> }

// GET /symbols/search?q=apple[&exchange=US] -> search symbols via Finnhub
async fn search_symbols(State(state): State<Arc<AppState>>, Query(params): Query<SymbolSearchQuery>) -> impl IntoResponse {
    let api_key = current_api_key(&state).await;
    if api_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"FINNHUB_API_KEY not configured"}))).into_response();
    }
    let q = params.q.trim();
    if q.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"missing q"}))).into_response();
    }
    let exchange = params.exchange.as_deref().unwrap_or("US");
    let client = reqwest::Client::new();
    let base = current_api_base(&state).await;
    match client
        .get(format!("{}/search", base.trim_end_matches('/')))
        .query(&[("q", q), ("exchange", exchange), ("token", api_key.as_str())])
        .send()
        .await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return (StatusCode::BAD_GATEWAY, Json(json!({"error":"finnhub search failed"}))).into_response();
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => {
                    let list = v.get("result").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                    let items: Vec<SymbolItem> = list
                        .into_iter()
                        .filter_map(|it| {
                            let symbol = it.get("symbol").and_then(|s| s.as_str()).unwrap_or("").to_string();
                            if symbol.is_empty() { return None; }
                            let description = it.get("description").and_then(|d| d.as_str()).map(|s| s.to_string());
                            Some(SymbolItem { symbol, description })
                        })
                        .collect();
                    Json(items).into_response()
                }
                Err(_) => (StatusCode::BAD_GATEWAY, Json(json!({"error":"invalid response"}))).into_response(),
            }
        }
        Err(_) => (StatusCode::BAD_GATEWAY, Json(json!({"error":"request failed"}))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SubscribeRequest { symbol: String }

// POST /instruments/subscribe { symbol }
async fn subscribe_instrument(State(state): State<Arc<AppState>>, Json(req): Json<SubscribeRequest>) -> impl IntoResponse {
    let api_key = current_api_key(&state).await;
    if api_key.is_empty() { return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"FINNHUB_API_KEY not configured"}))).into_response(); }

    let symbol = req.symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"symbol required"}))).into_response();
    }

    // Ensure instrument exists with a placeholder price
    let _ = sqlx::query("INSERT INTO instruments (symbol, price) VALUES ($1, $2) ON CONFLICT (symbol) DO NOTHING")
        .bind(&symbol)
        .bind(0.0f64)
        .execute(&state.db)
        .await;

    // No historical backfill: rely on live ticks to populate price_history and update current prices

    // Recompute and broadcast portfolio (prices may affect positions)
    let lots = compute_lots_from_db(&state.db).await;
    let prices = load_prices(&state.db).await;
    if let Ok(mut portfolio) = state.portfolio.lock() {
        let updated = build_portfolio_update_from_lots(&lots, &prices);
        *portfolio = updated.clone();
        let _ = state.tx.send(updated);
    }

    (StatusCode::CREATED, Json(json!({"status":"subscribed", "symbol": symbol }))).into_response()
}

#[derive(Debug, Serialize)]
struct TickOut { symbol: String, price: f64, ts: String }

#[derive(Debug, Deserialize)]
struct FinnhubTrade { p: f64, s: String, #[allow(dead_code)] t: Option<i64> }

#[derive(Debug, Deserialize)]
struct FinnhubMsg { #[serde(default)] r#type: String, #[serde(default)] data: Vec<FinnhubTrade> }

// GET /price-stream?symbols=AAPL,MSFT
async fn price_stream(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<PriceStreamQuery>) -> impl IntoResponse {
    // Optional header auth
    let expected = current_webhook_secret(&state).await;
    if !expected.is_empty() {
        let provided_header = headers
            .get("x-webhook-secret")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let provided_query = q.secret.as_deref().unwrap_or("");
        if provided_header != expected && provided_query != expected { return (StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    }

    let api_key = current_api_key(&state).await;
    if api_key.is_empty() { return (StatusCode::SERVICE_UNAVAILABLE, "FINNHUB_API_KEY not configured").into_response(); }

    let symbols: Vec<String> = q
        .symbols
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| { let t = s.trim(); if t.is_empty() { None } else { Some(t.to_string()) } })
        .collect();

    let db = state.db.clone();
    let stream = async_stream::stream! {
        // Connect to Finnhub WS
        let ws_base = current_ws_base(&state).await;
        let url = format!("{}?token={}", ws_base.trim_end_matches('/'), api_key);
        if let Ok((mut ws, _)) = connect_async(&url).await {
            // Subscribe symbols
            for s in &symbols {
                let msg = format!("{{\"type\":\"subscribe\",\"symbol\":\"{}\"}}", s);
                let _ = ws.send(tokio_tungstenite::tungstenite::Message::Text(msg)).await;
            }

            // Initial ack to client
            if let Ok(init) = serde_json::to_string(&json!({"status":"subscribed","symbols": symbols})) {
                let _ = yield Ok::<Event, Infallible>(Event::default().data(init));
            }

            while let Some(msg) = ws.next().await {
                match msg {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(txt)) => {
                        if let Ok(parsed) = serde_json::from_str::<FinnhubMsg>(&txt) {
                            if parsed.r#type == "trade" {
                                for t in parsed.data {
                                    let out = TickOut { symbol: t.s, price: t.p, ts: Utc::now().to_rfc3339() };
                                    // Persist tick
                                    let _ = sqlx::query("INSERT INTO price_history (symbol, price, ts) VALUES ($1, $2, NOW())")
                                        .bind(&out.symbol)
                                        .bind(out.price)
                                        .execute(&db)
                                        .await;
                                    let _ = sqlx::query("INSERT INTO instruments (symbol, price) VALUES ($1, $2) ON CONFLICT (symbol) DO UPDATE SET price = EXCLUDED.price")
                                        .bind(&out.symbol)
                                        .bind(out.price)
                                        .execute(&db)
                                        .await;
                                    if let Ok(data) = serde_json::to_string(&out) {
                                        let _ = yield Ok::<Event, Infallible>(Event::default().data(data));
                                    }
                                }
                            }
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => { break; }
                    Ok(_) => {}
                    Err(_) => { break; }
                }
            }
        } else {
            if let Ok(err) = serde_json::to_string(&json!({"error":"failed_to_connect_ws"})) {
                let _ = yield Ok::<Event, Infallible>(Event::default().data(err));
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(StdDuration::from_secs(15))
            .text("keep-alive-text"),
    ).into_response()
}

// ---- Price History ----
#[derive(Debug, Deserialize)]
struct HistoryQuery { days: Option<i64> }

#[derive(Debug, Serialize)]
struct HistoryPoint { timestamp: String, price: f64 }

// GET /instruments/:symbol/history?days=5
async fn get_price_history(State(state): State<Arc<AppState>>, Path(symbol): Path<String>, Query(q): Query<HistoryQuery>) -> impl IntoResponse {
    let days = q.days.unwrap_or(5).max(1);
    let since = Utc::now() - ChronoDuration::days(days);
    let rows = sqlx::query("SELECT price, ts FROM price_history WHERE symbol = $1 AND ts >= $2 ORDER BY ts ASC")
        .bind(&symbol)
        .bind(since)
        .fetch_all(&state.db)
        .await;
    match rows {
        Ok(rows) => {
            let data: Vec<HistoryPoint> = rows.into_iter().filter_map(|r| {
                let price: Option<f64> = r.try_get("price").ok();
                let ts: Option<chrono::DateTime<chrono::Utc>> = r.try_get("ts").ok();
                match (price, ts) { (Some(p), Some(t)) => Some(HistoryPoint { timestamp: t.to_rfc3339(), price: p }), _ => None }
            }).collect();
            (StatusCode::OK, Json(data)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("failed to fetch history: {}", e)}))).into_response()
    }
}

#[derive(Debug, Serialize)]
struct InstrumentItem {
    symbol: String,
    price: f64,
}

async fn load_prices(db: &Pool<Postgres>) -> HashMap<String, f64> {
    let mut map: HashMap<String, f64> = HashMap::new();
    if let Ok(rows) = sqlx::query("SELECT symbol, price FROM instruments")
        .fetch_all(db)
        .await
    {
        for row in rows {
            let symbol: String = row.get("symbol");
            let price: f64 = row.get("price");
            map.insert(symbol, price);
        }
    }
    map
}

// GET /instruments
async fn get_instruments(State(state): State<Arc<AppState>>) -> Response {
    let rows = sqlx::query("SELECT symbol, price FROM instruments ORDER BY symbol ASC")
        .fetch_all(&state.db)
        .await;
    match rows {
        Ok(rows) => {
            let items: Vec<InstrumentItem> = rows
                .into_iter()
                .map(|r| InstrumentItem { symbol: r.get("symbol"), price: r.get("price") })
                .collect();
            (StatusCode::OK, Json(items)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to fetch instruments: {}", e)})),
        ).into_response(),
    }
}

// Finnhub symbol item
#[derive(Debug, Deserialize, Serialize)]
struct SymbolItem { symbol: String, description: Option<String> }

// GET /symbols -> list of symbols from Finnhub (US exchange by default)
async fn get_symbols(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let api_key = current_api_key(&state).await;
    if api_key.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"FINNHUB_API_KEY not configured"}))).into_response();
    }
    let base = current_api_base(&state).await;
    let url = format!("{}/stock/symbol?exchange=US&token={}", base.trim_end_matches('/'), api_key);
    let client = reqwest::Client::new();
    match client.get(&url).send().await.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json::<Vec<serde_json::Value>>().await {
            Ok(list) => {
                let items: Vec<SymbolItem> = list.into_iter().filter_map(|v| {
                    let symbol = v.get("symbol").and_then(|s| s.as_str())?.to_string();
                    let description = v.get("description").and_then(|d| d.as_str()).map(|s| s.to_string());
                    Some(SymbolItem { symbol, description })
                }).collect();
                (StatusCode::OK, Json(items)).into_response()
            }
            Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({"error": format!("parse error: {}", e)}))).into_response(),
        },
        Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({"error": format!("request failed: {}", e)}))).into_response(),
    }
}

// Removed: backfill_price_history endpoint and related types

// DELETE /instruments/:symbol
async fn delete_instrument(State(state): State<Arc<AppState>>, Path(symbol): Path<String>) -> impl IntoResponse {
    // Prevent deletion if there are still open positions (non-zero lots) for this symbol
    let lots = compute_lots_from_db(&state.db).await;
    if let Some(entries) = lots.get(&symbol) {
        let has_qty = entries.iter().any(|(q, _)| *q > f64::EPSILON);
        if has_qty {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "cannot delete instrument with open positions",
                    "symbol": symbol
                })),
            );
        }
    }

    // Proceed with deletion if no open positions
    let res = sqlx::query("DELETE FROM instruments WHERE symbol = $1")
        .bind(&symbol)
        .execute(&state.db)
        .await;
    match res {
        Ok(_) => {
            // Rebuild with prices after deletion
            let lots = compute_lots_from_db(&state.db).await;
            let prices = load_prices(&state.db).await;
            if let Ok(mut portfolio) = state.portfolio.lock() {
                let updated = build_portfolio_update_from_lots(&lots, &prices);
                *portfolio = updated.clone();
                let _ = state.tx.send(updated);
            }
            (StatusCode::NO_CONTENT, Json(serde_json::json!({"status": "deleted"})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to delete: {}", e)})),
        ),
    }
}
use uuid::Uuid;

// Application state
#[derive(Clone)]
struct AppState {
    tx: Sender<PortfolioUpdate>,
    // In-memory portfolio state (protected by Mutex for interior mutability)
    portfolio: Arc<Mutex<PortfolioUpdate>>, 
    // Database pool for persistence
    db: Pool<Postgres>,
    // Provider configuration (persisted, hot-reloadable)
    provider_config: Arc<tokio::sync::RwLock<ProviderConfig>>, 
}

// Utilities to rebuild individual lots (positions) from transaction history
// Each BUY creates a lot; SELL reduces quantities from existing lots FIFO.
async fn compute_lots_from_db(db: &Pool<Postgres>) -> HashMap<String, Vec<(f64, f64)>> {
    // Returns symbol -> Vec<(quantity, avg_cost_per_lot)>
    let mut lots: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    let rows = sqlx::query(
        "SELECT type, symbol, quantity, price, timestamp, id FROM transactions ORDER BY timestamp ASC, id ASC",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    for row in rows {
        let t: String = row.get::<String, _>("type");
        let symbol: String = row.get::<String, _>("symbol");
        let qty: f64 = row.get::<f64, _>("quantity");
        let price: f64 = row.try_get("price").ok().flatten().unwrap_or(0.0);

        let entry = lots.entry(symbol).or_default();
        match t.as_str() {
            "BUY" => {
                // Add a new lot
                if qty > 0.0 {
                    entry.push((qty, price));
                }
            }
            "SELL" => {
                // Reduce FIFO
                let mut to_sell = qty.max(0.0);
                let mut i = 0usize;
                while to_sell > 0.0 && i < entry.len() {
                    let (ref mut lot_qty, _lot_price) = entry[i];
                    if *lot_qty <= to_sell + f64::EPSILON {
                        to_sell -= *lot_qty;
                        *lot_qty = 0.0;
                        i += 1;
                    } else {
                        *lot_qty -= to_sell;
                        to_sell = 0.0;
                    }
                }
                // Remove depleted lots
                entry.retain(|(q, _)| *q > f64::EPSILON);
            }
            _ => { /* ignore unknown types */ }
        }
    }
    lots
}

fn build_portfolio_update_from_lots(
    lots: &HashMap<String, Vec<(f64, f64)>>,
    existing_prices: &HashMap<String, f64>,
) -> PortfolioUpdate {
    let mut positions: Vec<Position> = Vec::new();
    for (symbol, lot_list) in lots.iter() {
        for (qty, avg) in lot_list {
            if *qty <= 0.0 { continue; }
            let price = existing_prices.get(symbol).copied().unwrap_or(0.0);
            let value = price * *qty;
            let pnl = (price - *avg) * *qty;
            let pnl_percent = if *avg > 0.0 { (price - *avg) / *avg * 100.0 } else { 0.0 };
            positions.push(Position {
                symbol: symbol.clone(),
                quantity: *qty,
                price,
                value,
                average_cost: *avg,
                pnl,
                pnl_percent,
            });
        }
    }
    let portfolio_value = positions.iter().map(|p| p.value).sum();
    PortfolioUpdate { timestamp: Utc::now().to_rfc3339(), portfolio_value, positions }
}

// Handler for GET /transactions
async fn get_transactions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rows = sqlx::query(
        "SELECT id, type, symbol, quantity, price, timestamp FROM transactions ORDER BY timestamp DESC LIMIT 200"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<Transaction> = rows
        .into_iter()
        .filter_map(|row| {
            let id: Option<Uuid> = row.try_get("id").ok();
            let t: Option<String> = row.try_get("type").ok();
            let symbol: Option<String> = row.try_get("symbol").ok();
            let quantity: Option<f64> = row.try_get("quantity").ok();
            let price: Option<f64> = row.try_get("price").ok();
            let ts: Option<chrono::DateTime<chrono::Utc>> = row.try_get("timestamp").ok();
            match (id, t, symbol, quantity, ts) {
                (Some(id), Some(t), Some(symbol), Some(quantity), Some(ts)) => Some(Transaction {
                    id: id.to_string(),
                    r#type: t,
                    symbol,
                    quantity,
                    price,
                    timestamp: ts.to_rfc3339(),
                }),
                _ => None,
            }
        })
        .collect();

    (StatusCode::OK, Json(items))
}

// Handler for POST /transactions
async fn add_transaction(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddTransactionRequest>,
) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let ts = req
        .timestamp
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc)))
        .unwrap_or_else(|| Utc::now());

    let _ = sqlx::query(
        "INSERT INTO transactions (id, type, symbol, quantity, price, timestamp) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(id)
    .bind(&req.r#type)
    .bind(&req.symbol)
    .bind(req.quantity)
    .bind(req.price)
    .bind(ts)
    .execute(&state.db)
    .await;

    let tx = Transaction {
        id: id.to_string(),
        r#type: req.r#type,
        symbol: req.symbol,
        quantity: req.quantity,
        price: req.price,
        timestamp: ts.to_rfc3339(),
    };
    // Rebuild positions from DB, preserving existing prices per symbol
    let lots = compute_lots_from_db(&state.db).await;
    let prices = load_prices(&state.db).await;
    if let Ok(mut portfolio) = state.portfolio.lock() {
        let updated = build_portfolio_update_from_lots(&lots, &prices);
        *portfolio = updated.clone();
        let _ = state.tx.send(updated);
    }
    (StatusCode::CREATED, Json(tx))
}

// Handler for DELETE /transactions (clear all)
async fn clear_transactions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Delete all rows
    let res = sqlx::query("DELETE FROM transactions")
        .execute(&state.db)
        .await;

    match res {
        Ok(_) => {
            // Rebuild from empty DB with current instrument prices
            let lots = compute_lots_from_db(&state.db).await;
            let prices = load_prices(&state.db).await;
            if let Ok(mut portfolio) = state.portfolio.lock() {
                let updated = build_portfolio_update_from_lots(&lots, &prices);
                *portfolio = updated.clone();
                let _ = state.tx.send(updated);
            }
            (StatusCode::NO_CONTENT, Json(serde_json::json!({ "status": "cleared" })))
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("failed to clear: {}", e) })),
            )
        }
    }
}
// Portfolio update message for SSE
#[derive(Debug, Clone, Serialize)]
struct PortfolioUpdate {
    timestamp: String,
    portfolio_value: f64,
    positions: Vec<Position>,
}

// Position in the portfolio
#[derive(Debug, Clone, Serialize)]
struct Position {
    symbol: String,
    quantity: f64,
    price: f64,
    value: f64,
    average_cost: f64,
    pnl: f64,
    pnl_percent: f64,
}

// Transaction log entry persisted in-memory (and served to clients)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Transaction {
    id: String,
    r#type: String, // "BUY" | "SELL"
    symbol: String,
    quantity: f64,
    price: Option<f64>,
    timestamp: String,
}

// (Removed old manual update-price types)

#[derive(Debug, Deserialize)]
struct AddPositionRequest {
    symbol: String,
    quantity: f64,
    average_cost: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct UpdatePositionRequest {
    quantity: f64,
}

#[derive(Debug, Deserialize)]
struct AddTransactionRequest {
    r#type: String,
    symbol: String,
    quantity: f64,
    price: Option<f64>,
    // allow client to provide timestamp, otherwise server will set
    timestamp: Option<String>,
}

// Recalculate portfolio_value from positions
fn recalc_portfolio_value(p: &mut PortfolioUpdate) {
    p.portfolio_value = p.positions.iter().map(|pos| pos.value).sum();
}

// Handler for DELETE /portfolio/positions/{position_id}
async fn delete_position(
    axum::extract::Path(position_id): axum::extract::Path<String>,
    state: State<Arc<AppState>>,
) -> impl IntoResponse {
    info!("Deleting position: {}", position_id);
    // Remove any positions matching the provided identifier (treat as symbol for now)
    let mut removed_count = 0usize;
    if let Ok(mut portfolio) = state.portfolio.lock() {
        let before = portfolio.positions.len();
        portfolio.positions.retain(|p| p.symbol != position_id);
        removed_count = before - portfolio.positions.len();
        if removed_count > 0 {
            portfolio.timestamp = Utc::now().to_rfc3339();
            recalc_portfolio_value(&mut portfolio);
        }
        // Broadcast updated portfolio regardless
        let _ = state.tx.send(portfolio.clone());
    }

    let response = json!({
        "position_id": position_id,
        "removed": removed_count,
        "status": if removed_count > 0 { "deleted" } else { "not_found" }
    });

    (StatusCode::OK, Json(response))
}

// Handler for PUT /portfolio/positions/{position_id}
async fn update_position(
    axum::extract::Path(position_id): axum::extract::Path<String>,
    state: State<Arc<AppState>>,
    Json(payload): Json<UpdatePositionRequest>,
) -> impl IntoResponse {
    info!("Updating position {} with quantity: {}", position_id, payload.quantity);
    
    // In a real implementation, we would:
    // 1. Find the position by ID
    // 2. Update its quantity
    // 3. Return the updated position
    
    // For now, just return a success response
    let response = json!({
        "position_id": position_id,
        "quantity": payload.quantity,
        "status": "updated"
    });
    
    // Broadcast current state (no-op placeholder until update by ID is implemented)
    if let Ok(locked) = state.portfolio.lock() {
        let _ = state.tx.send(locked.clone());
    }
    
    (StatusCode::OK, Json(response))
}

// Handler for POST /portfolio/positions
async fn add_position(
    state: State<Arc<AppState>>,
    Json(payload): Json<AddPositionRequest>,
) -> impl IntoResponse {
    // In a real implementation, we would:
    // 1. Validate the request
    // 2. Add the position to the portfolio
    // 3. Return the updated portfolio or position ID
    
    // For now, we'll just log the request and return a success response
    info!("Adding position: {:?}", payload);
    
    // Generate a position and add it to the in-memory portfolio
    let position_id = Uuid::new_v4().to_string();
    {
        if let Ok(mut portfolio) = state.portfolio.lock() {
            // Default new positions to price 0 and value 0 until a price is provided
            let average_cost = payload.average_cost.unwrap_or(0.0);
            let price = 0.0;
            let quantity = payload.quantity;
            let value = quantity * price;
            let pnl = (price - average_cost) * quantity;
            let pnl_percent = if average_cost > 0.0 { (price - average_cost) / average_cost * 100.0 } else { 0.0 };
            let pos = Position {
                symbol: payload.symbol.clone(),
                quantity,
                price,
                value,
                average_cost,
                pnl,
                pnl_percent,
            };
            portfolio.positions.push(pos);
            portfolio.timestamp = Utc::now().to_rfc3339();
            recalc_portfolio_value(&mut portfolio);
        }
    }

    // Build response
    let response = json!({
        "position_id": position_id,
        "symbol": payload.symbol,
        "quantity": payload.quantity,
        "average_cost": payload.average_cost,
        "status": "added"
    });

    // Broadcast updated portfolio to SSE subscribers
    if let Ok(locked) = state.portfolio.lock() {
        let _ = state.tx.send(locked.clone());
    }
    
    (StatusCode::CREATED, Json(response))
}

// Handler for GET /portfolio/analysis/performance
async fn get_portfolio_performance(_state: State<Arc<AppState>>) -> impl IntoResponse {
    // In a real implementation, we would calculate these metrics based on the portfolio
    let response = json!({
        "total_return": 150_000.0,
        "total_return_percentage": 15.0,  // 15% return
        "annualized_return": 0.18,  // 18% annualized
        "ytd_return": 0.12,  // 12% YTD
        "monthly_returns": [
            0.02, 0.015, -0.01, 0.03, 0.01,  // Last 5 months
        ],
        "sharpe_ratio": 1.2,
        "sortino_ratio": 1.5,
        "alpha": 0.02,  // 2% alpha
        "beta": 1.05,
        "r_squared": 0.95,
        "tracking_error": 0.08,
        "information_ratio": 0.25,
        "max_drawdown": 0.15,  // 15%
        "calmar_ratio": 1.2,
        "start_date": "2024-01-01T00:00:00Z",
        "end_date": Utc::now().to_rfc3339(),
    });
    
    (StatusCode::OK, Json(response))
}

// Handler for GET /portfolio/analysis/risk
async fn get_portfolio_risk(_state: State<Arc<AppState>>) -> impl IntoResponse {
    // In a real implementation, we would calculate these metrics based on the portfolio
    let response = json!({
        "portfolio_value": 1_000_000.0,
        "value_at_risk_1d_95": 25_000.0,  // 2.5% of portfolio
        "value_at_risk_10d_95": 75_000.0, // 7.5% of portfolio
        "expected_shortfall_95": 35_000.0,
        "volatility_1y": 0.20,  // 20% annualized
        "beta": 1.05,
        "sharpe_ratio": 1.2,
        "sortino_ratio": 1.5,
        "max_drawdown": 0.15,  // 15%
        "last_updated": Utc::now().to_rfc3339(),
    });
    
    (StatusCode::OK, Json(response))
}

// Handler for GET /portfolio
async fn get_portfolio(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Return current in-memory portfolio
    let body = if let Ok(locked) = state.portfolio.lock() {
        locked.clone()
    } else {
        PortfolioUpdate { timestamp: Utc::now().to_rfc3339(), portfolio_value: 0.0, positions: vec![] }
    };
    Json(body)
}

// (Removed old manual update-price handler)

// Handler for GET /stream
async fn stream_updates(State(state): State<Arc<AppState>>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = async_stream::stream! {
        // Send an initial snapshot of the current in-memory portfolio
        let initial = state.portfolio.lock().ok().map(|p| p.clone());
        if let Some(initial) = initial {
            if let Ok(data) = serde_json::to_string(&initial) {
                yield Ok(Event::default().data(data));
            }
        }

        // Then forward broadcast updates as they arrive
        let mut rx = BroadcastStream::new(rx);
        while let Some(Ok(update)) = rx.next().await {
            match serde_json::to_string(&update) {
                Ok(data) => yield Ok(Event::default().data(data)),
                Err(_) => continue,
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(StdDuration::from_secs(15))
            .text("keep-alive-text"),
    )
}

// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // Create broadcast channel for SSE
    let (tx, _) = broadcast::channel(100);
    // Initialize Postgres connection pool
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/valuation".to_string());
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    // Create tables if they don't exist
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS transactions (\n            id UUID PRIMARY KEY,\n            type TEXT NOT NULL,\n            symbol TEXT NOT NULL,\n            quantity DOUBLE PRECISION NOT NULL,\n            price DOUBLE PRECISION,\n            timestamp TIMESTAMPTZ NOT NULL\n        )"
    )
    .execute(&db)
    .await;

    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS instruments (\n            symbol TEXT PRIMARY KEY,\n            price DOUBLE PRECISION NOT NULL\n        )"
    )
    .execute(&db)
    .await;

    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS price_history (\n            id BIGSERIAL PRIMARY KEY,\n            symbol TEXT NOT NULL,\n            price DOUBLE PRECISION NOT NULL,\n            ts TIMESTAMPTZ NOT NULL DEFAULT NOW()\n        )"
    )
    .execute(&db)
    .await;

    // Ensure provider config table exists and load config
    ensure_provider_config_table(&db).await;

    // Build initial in-memory portfolio from persisted transactions (as individual lots)
    let lots = compute_lots_from_db(&db).await;
    let prices = load_prices(&db).await;
    let initial_from_db = build_portfolio_update_from_lots(&lots, &prices);

    let state = Arc::new(AppState {
        tx,
        portfolio: Arc::new(Mutex::new(initial_from_db)),
        db: db.clone(),
        provider_config: Arc::new(tokio::sync::RwLock::new(load_provider_config(&db).await)),
    });

    // Set up CORS
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_headers([header::CONTENT_TYPE, header::HeaderName::from_static("x-webhook-secret")])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ]);

    // Build our application with routes
    let app = Router::new()
        // System
        .route("/health", get(health_check))
        
        // Portfolio Management
        .route("/portfolio", get(get_portfolio))
        .route("/portfolio/positions", post(add_position))
        .route("/portfolio/positions/:position_id", put(update_position).delete(delete_position))
        // Transactions
        .route("/transactions", get(get_transactions).post(add_transaction).delete(clear_transactions))
        // Instruments (read-only history; manual updates removed)
        .route("/instruments", get(get_instruments))
        .route("/instruments/subscribe", post(subscribe_instrument))
        .route("/instruments/:symbol", delete(delete_instrument))
        .route("/instruments/:symbol/history", get(get_price_history))
        // Symbols universe and backfill
        .route("/symbols", get(get_symbols))
        .route("/symbols/search", get(search_symbols))
        // Backfill route removed
        
        // Admin configuration
        .route("/admin/provider-config", get(get_provider_config).put(update_provider_config))
        
        // Portfolio Analysis
        .route("/portfolio/analysis/risk", get(get_portfolio_risk))
        .route("/portfolio/analysis/performance", get(get_portfolio_performance))
        
        // Market Data: manual update removed
        
        // Real-time Updates
        .route("/stream", get(stream_updates))
        // Price streaming proxy to Finnhub
        .route("/price-stream", get(price_stream))
        
        .with_state(state)
        .layer(cors);

    // Start the server
    let addr: std::net::SocketAddr = "0.0.0.0:3000".parse().unwrap();
    info!("Server listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
