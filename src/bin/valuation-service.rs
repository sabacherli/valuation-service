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
use std::{convert::Infallible, sync::Arc, time::Duration};
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

// Generate a sample portfolio update
async fn generate_portfolio_update() -> PortfolioUpdate {
    PortfolioUpdate {
        timestamp: Utc::now().to_rfc3339(),
        portfolio_value: 1_000_000.0,
        positions: vec![
            Position {
                symbol: "AAPL".to_string(),
                quantity: 100.0,
                price: 180.0,
                value: 18_000.0,
            },
            Position {
                symbol: "MSFT".to_string(),
                quantity: 50.0,
                price: 300.0,
                value: 15_000.0,
            },
        ],
    }
}

// Handler for DELETE /portfolio/positions/{position_id}
async fn delete_position(
    axum::extract::Path(position_id): axum::extract::Path<String>,
    state: State<Arc<AppState>>,
) -> impl IntoResponse {
    info!("Deleting position: {}", position_id);
    
    // In a real implementation, we would:
    // 1. Find the position by ID
    // 2. Remove it from the portfolio
    
    // For now, just return a success response
    let response = json!({
        "position_id": position_id,
        "status": "deleted"
    });
    
    // Send update to SSE subscribers
    let update = generate_portfolio_update().await;
    let _ = state.tx.send(update);
    
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
    
    // Send update to SSE subscribers
    let update = generate_portfolio_update().await;
    let _ = state.tx.send(update);
    
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
    
    // Generate a mock response
    let response = json!({
        "position_id": Uuid::new_v4().to_string(),
        "symbol": payload.symbol,
        "quantity": payload.quantity,
        "average_cost": payload.average_cost,
        "status": "added"
    });
    
    // Send update to SSE subscribers
    let update = generate_portfolio_update().await;
    let _ = state.tx.send(update);
    
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
async fn get_portfolio(_state: State<Arc<AppState>>) -> impl IntoResponse {
    let update = generate_portfolio_update().await;
    Json(update)
}

// Handler for POST /update-price
async fn update_price(
    state: State<Arc<AppState>>,
    Json(update_req): Json<UpdatePriceRequest>,
) -> impl IntoResponse {
    info!("Updating price for {} to {}", update_req.symbol, update_req.price);
    
    // In a real implementation, we would update the price in our data store
    // For now, we'll just log it and generate a new portfolio update
    
    let update = generate_portfolio_update().await;
    let _ = state.tx.send(update.clone());
    
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
        // Send an initial snapshot immediately upon connection
        let initial = generate_portfolio_update().await;
        if let Ok(data) = serde_json::to_string(&initial) {
            yield Ok(Event::default().data(data));
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
    let state = Arc::new(AppState { tx });

    // Set up CORS
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_headers([header::CONTENT_TYPE])
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST]);

    // Build our application with routes
    let app = Router::new()
        // System
        .route("/health", get(health_check))
        
        // Portfolio Management
        .route("/portfolio", get(get_portfolio))
        .route("/portfolio/positions", post(add_position))
        .route("/portfolio/positions/:position_id", put(update_position).delete(delete_position))
        
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
