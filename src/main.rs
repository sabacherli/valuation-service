use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::info;

use valuation_service::{
    instruments::Stock,
    market_data::{MockMarketDataProvider, MarketDataProvider},
    models::BlackScholesModel,
    portfolio::{Portfolio, PortfolioValuationService},
    risk::RiskEngine,
    valuation::Instrument,
};

// Simplified request/response types
#[derive(Deserialize)]
struct CreatePortfolioRequest {
    name: String,
    base_currency: String,
}

#[derive(Deserialize)]
struct AddPositionRequest {
    instrument_id: String,
    quantity: f64,
    average_cost: Option<f64>,
}

#[derive(Deserialize)]
struct CreateStockRequest {
    symbol: String,
    currency: String,
    shares: f64,
}

#[derive(Serialize)]
struct SimpleResponse {
    success: bool,
    message: String,
    data: Option<serde_json::Value>,
}

// Application state
#[derive(Clone)]
struct AppState {
    portfolios: Arc<RwLock<HashMap<String, Portfolio>>>,
    instruments: Arc<RwLock<HashMap<String, Box<dyn Instrument + Send + Sync>>>>,
    market_data_provider: Arc<MockMarketDataProvider>,
    portfolio_service: Arc<PortfolioValuationService>,
    black_scholes_model: Arc<BlackScholesModel>,
}

// Simplified handlers that return SimpleResponse
async fn health_check() -> Json<SimpleResponse> {
    Json(SimpleResponse {
        success: true,
        message: "Valuation Service is healthy".to_string(),
        data: None,
    })
}

async fn create_portfolio(
    State(state): State<AppState>,
    Json(request): Json<CreatePortfolioRequest>,
) -> Result<Json<SimpleResponse>, StatusCode> {
    let portfolio = Portfolio::new(request.name, request.base_currency);
    let portfolio_id = portfolio.id.clone();
    
    state.portfolios.write().await.insert(portfolio_id.clone(), portfolio);
    
    info!("Created portfolio: {}", portfolio_id);
    Ok(Json(SimpleResponse {
        success: true,
        message: "Portfolio created successfully".to_string(),
        data: Some(serde_json::json!({"portfolio_id": portfolio_id})),
    }))
}

async fn get_portfolio(
    State(state): State<AppState>,
    Path(portfolio_id): Path<String>,
) -> Result<Json<SimpleResponse>, StatusCode> {
    let portfolios = state.portfolios.read().await;
    match portfolios.get(&portfolio_id) {
        Some(portfolio) => Ok(Json(SimpleResponse {
            success: true,
            message: "Portfolio retrieved successfully".to_string(),
            data: Some(serde_json::to_value(portfolio).unwrap()),
        })),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn create_stock(
    State(state): State<AppState>,
    Json(request): Json<CreateStockRequest>,
) -> Result<Json<SimpleResponse>, StatusCode> {
    let stock = Stock::new(request.symbol.clone(), request.currency, request.shares);
    let instrument_id = stock.id().to_string();
    
    state.instruments.write().await.insert(
        instrument_id.clone(),
        Box::new(stock),
    );
    
    info!("Created stock instrument: {}", instrument_id);
    Ok(Json(SimpleResponse {
        success: true,
        message: "Stock created successfully".to_string(),
        data: Some(serde_json::json!({"instrument_id": instrument_id})),
    }))
}

async fn get_market_data(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Json<SimpleResponse>, StatusCode> {
    match state.market_data_provider.get_market_context(&symbol).await {
        Ok(context) => Ok(Json(SimpleResponse {
            success: true,
            message: "Market data retrieved successfully".to_string(),
            data: Some(serde_json::to_value(context).unwrap()),
        })),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[axum::debug_handler]
async fn value_portfolio(
    State(state): State<AppState>,
    Path(portfolio_id): Path<String>,
) -> Result<Json<SimpleResponse>, StatusCode> {
    let portfolios = state.portfolios.read().await;
    let portfolio = match portfolios.get(&portfolio_id) {
        Some(p) => p,
        None => return Err(StatusCode::NOT_FOUND),
    };
    
    let instruments = state.instruments.read().await;
    
    // Get market context for the first instrument (simplified)
    let market_context = match state.market_data_provider.get_market_context("AAPL").await {
        Ok(ctx) => ctx,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    // Value portfolio
    match state.portfolio_service.value_portfolio(
        portfolio,
        &*instruments,
        state.black_scholes_model.as_ref(),
        &market_context,
    ).await {
        Ok(valuation) => Ok(Json(SimpleResponse {
            success: true,
            message: "Portfolio valued successfully".to_string(),
            data: Some(serde_json::to_value(valuation).unwrap()),
        })),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Initialize services
    let market_data_provider = Arc::new(MockMarketDataProvider::new());
    let risk_engine = RiskEngine::new(0.95, 1, 10000);
    let portfolio_service = Arc::new(PortfolioValuationService::new(risk_engine));
    let black_scholes_model = Arc::new(BlackScholesModel::new());
    
    // Initialize state
    let state = AppState {
        portfolios: Arc::new(RwLock::new(HashMap::new())),
        instruments: Arc::new(RwLock::new(HashMap::new())),
        market_data_provider,
        portfolio_service,
        black_scholes_model,
    };
    
    // Build application with routes
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/portfolios", post(create_portfolio))
        .route("/portfolios/:id", get(get_portfolio))
        .route("/portfolios/:id/valuation", get(value_portfolio))
        .route("/instruments/stocks", post(create_stock))
        .route("/market-data/:symbol", get(get_market_data))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive())
        )
        .with_state(state);
    
    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    info!("🚀 Valuation Service starting on http://0.0.0.0:3000");
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
