use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{sse::Event, IntoResponse, Sse},
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use std::{convert::Infallible, sync::{Arc, Mutex}, time::Duration};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, Row};
use std::env;
use tokio::sync::broadcast::{self, Sender};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

// Application state
#[derive(Clone)]
struct AppState {
    tx: Sender<PortfolioUpdate>,
    // In-memory portfolio state (protected by Mutex for interior mutability)
    portfolio: Arc<Mutex<PortfolioUpdate>>, 
    // Database pool for persistence
    db: Pool<Postgres>,
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
    (StatusCode::CREATED, Json(tx))
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

// Request for updating a stock price
#[derive(Debug, Deserialize)]
struct UpdatePriceRequest {
    symbol: String,
    price: f64,
}

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

// Handler for POST /update-price
async fn update_price(
    state: State<Arc<AppState>>,
    Json(update_req): Json<UpdatePriceRequest>,
) -> impl IntoResponse {
    info!("Updating price for {} to {}", update_req.symbol, update_req.price);
    
    // Update price in the in-memory portfolio if symbol exists
    if let Ok(mut portfolio) = state.portfolio.lock() {
        for pos in &mut portfolio.positions {
            if pos.symbol == update_req.symbol {
                pos.price = update_req.price;
                pos.value = pos.quantity * pos.price;
                pos.pnl = (pos.price - pos.average_cost) * pos.quantity;
                pos.pnl_percent = if pos.average_cost > 0.0 { (pos.price - pos.average_cost) / pos.average_cost * 100.0 } else { 0.0 };
            }
        }
        portfolio.timestamp = Utc::now().to_rfc3339();
        recalc_portfolio_value(&mut portfolio);
        let _ = state.tx.send(portfolio.clone());
    }
    
    (StatusCode::OK, Json(json!({
        "status": "price_updated",
        "symbol": update_req.symbol,
        "new_price": update_req.price,
        "timestamp": Utc::now().to_rfc3339()
    })))
}

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
            .interval(Duration::from_secs(15))
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
    // Initialize empty in-memory portfolio
    let initial_portfolio = PortfolioUpdate {
        timestamp: Utc::now().to_rfc3339(),
        portfolio_value: 0.0,
        positions: vec![],
    };
    // Initialize Postgres connection pool
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/valuation".to_string());
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    // Create transactions table if it doesn't exist
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS transactions (\n            id UUID PRIMARY KEY,\n            type TEXT NOT NULL,\n            symbol TEXT NOT NULL,\n            quantity DOUBLE PRECISION NOT NULL,\n            price DOUBLE PRECISION,\n            timestamp TIMESTAMPTZ NOT NULL\n        )"
    )
    .execute(&db)
    .await;

    let state = Arc::new(AppState {
        tx,
        portfolio: Arc::new(Mutex::new(initial_portfolio)),
        db,
    });

    // Set up CORS
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_headers([header::CONTENT_TYPE])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
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
        .route("/transactions", get(get_transactions).post(add_transaction))
        
        // Portfolio Analysis
        .route("/portfolio/analysis/risk", get(get_portfolio_risk))
        .route("/portfolio/analysis/performance", get(get_portfolio_performance))
        
        // Market Data
        .route("/update-price", post(update_price))
        
        // Real-time Updates
        .route("/stream", get(stream_updates))
        
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
