use axum::{
    extract::Query,
    http::{StatusCode, header, HeaderMap, HeaderValue},
    response::{Json, Response},
    routing::get,
    Router,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::time::{interval, Duration as TokioDuration};
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use valuation_service::{
    instruments::{FinancialOption, OptionType, ExerciseStyle, Stock},
    market_data::{MockMarketDataProvider, MarketDataProvider},
    models::BlackScholesModel,
    portfolio::{Portfolio, PortfolioValuationService},
    risk::RiskEngine,
    valuation::{Instrument, Valuator},
};

#[derive(Debug, Serialize)]
struct PortfolioResponse {
    total_value: f64,
    total_pnl: f64,
    total_var: f64,
    portfolio_volatility: f64,
    sharpe_ratio: f64,
    max_drawdown: f64,
    positions: Vec<PositionResponse>,
    greeks: GreeksResponse,
    exposures: ExposuresResponse,
}

#[derive(Debug, Serialize)]
struct PositionResponse {
    instrument_id: String,
    instrument_type: String,
    symbol: String,
    quantity: f64,
    market_value: f64,
    pnl: f64,
    weight: f64,
    delta: f64,
    gamma: f64,
    theta: f64,
    vega: f64,
    rho: f64,
}

#[derive(Debug, Serialize)]
struct GreeksResponse {
    total_delta: f64,
    total_gamma: f64,
    total_theta: f64,
    total_vega: f64,
    total_rho: f64,
}

#[derive(Debug, Serialize)]
struct ExposuresResponse {
    by_instrument_type: HashMap<String, f64>,
    by_underlying: HashMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct PortfolioQuery {
    #[serde(default)]
    include_greeks: bool,
    #[serde(default)]
    include_risk: bool,
}

async fn get_portfolio_data(Query(params): Query<PortfolioQuery>) -> Result<Json<PortfolioResponse>, StatusCode> {
    // Initialize services
    let market_data = MockMarketDataProvider::new();
    let black_scholes = BlackScholesModel::new();
    let risk_engine = RiskEngine::new(0.95, 1, 10000);
    let portfolio_service = PortfolioValuationService::new(risk_engine);
    
    // Create sample instruments
    let aapl_stock = Stock::new("AAPL".to_string(), "USD".to_string(), 100.0);
    let aapl_call = FinancialOption::new(
        "AAPL".to_string(),
        "USD".to_string(),
        OptionType::Call,
        180.0,
        Utc::now() + Duration::days(30),
        10.0,
        ExerciseStyle::European,
    );
    
    // Create portfolio
    let mut portfolio = Portfolio::new("Demo Portfolio".to_string(), "USD".to_string());
    portfolio.add_position(aapl_stock.id().to_string(), 100.0, Some(175.00));
    portfolio.add_position(aapl_call.id().to_string(), 10.0, Some(5.50));
    
    // Prepare instruments map
    let mut instruments = HashMap::new();
    instruments.insert(aapl_stock.id().to_string(), Box::new(aapl_stock) as Box<dyn Instrument + Send + Sync>);
    instruments.insert(aapl_call.id().to_string(), Box::new(aapl_call) as Box<dyn Instrument + Send + Sync>);
    
    // Get market context
    let market_context = market_data.get_market_context("AAPL").await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Value portfolio
    let valuation_result = portfolio_service.value_portfolio(&portfolio, &instruments, &black_scholes, &market_context).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Calculate Greeks for each position
    let mut positions = Vec::new();
    let mut total_delta = 0.0;
    let mut total_gamma = 0.0;
    let mut total_theta = 0.0;
    let mut total_vega = 0.0;
    let mut total_rho = 0.0;
    
    for position in portfolio.positions.iter() {
        if let Some(instrument) = instruments.get(&position.instrument_id) {
            let position_value = valuation_result.positions.iter()
                .find(|p| p.instrument_id == position.instrument_id)
                .map(|p| p.total_value)
                .unwrap_or(0.0);
            let cost_basis = position.average_cost.unwrap_or(0.0) * position.quantity;
            let pnl = position_value - cost_basis;
            let weight = (position_value / valuation_result.total_value) * 100.0;
            
            // Calculate Greeks if requested
            let (delta, gamma, theta, vega, rho) = if params.include_greeks {
                let greeks = black_scholes.calculate_greeks(instrument.as_ref(), &market_context)
                    .unwrap_or_else(|_| valuation_service::valuation::Greeks {
                        delta: Some(0.0),
                        gamma: Some(0.0),
                        theta: Some(0.0),
                        vega: Some(0.0),
                        rho: Some(0.0),
                    });
                
                let pos_delta = greeks.delta.unwrap_or(0.0) * position.quantity;
                let pos_gamma = greeks.gamma.unwrap_or(0.0) * position.quantity;
                let pos_theta = greeks.theta.unwrap_or(0.0) * position.quantity;
                let pos_vega = greeks.vega.unwrap_or(0.0) * position.quantity;
                let pos_rho = greeks.rho.unwrap_or(0.0) * position.quantity;
                
                total_delta += pos_delta;
                total_gamma += pos_gamma;
                total_theta += pos_theta;
                total_vega += pos_vega;
                total_rho += pos_rho;
                
                (pos_delta, pos_gamma, pos_theta, pos_vega, pos_rho)
            } else {
                (0.0, 0.0, 0.0, 0.0, 0.0)
            };
            
            let instrument_type = match instrument.instrument_type() {
                valuation_service::valuation::InstrumentType::Stock => "Stock",
                valuation_service::valuation::InstrumentType::Option => "Option",
                _ => "Other",
            };
            
            let symbol = if instrument_type == "Stock" {
                "AAPL".to_string()
            } else {
                "AAPL Call $180".to_string()
            };
            
            positions.push(PositionResponse {
                instrument_id: position.instrument_id.clone(),
                instrument_type: instrument_type.to_string(),
                symbol,
                quantity: position.quantity,
                market_value: position_value,
                pnl,
                weight,
                delta,
                gamma,
                theta,
                vega,
                rho,
            });
        }
    }
    
    // Calculate exposures
    let mut by_instrument_type = HashMap::new();
    let mut by_underlying = HashMap::new();
    
    for position in &positions {
        *by_instrument_type.entry(position.instrument_type.clone()).or_insert(0.0) += position.market_value;
        *by_underlying.entry("AAPL".to_string()).or_insert(0.0) += position.market_value;
    }
    
    // Calculate risk metrics
    let var_1d = if params.include_risk {
        valuation_result.risk_metrics.as_ref().and_then(|r| r.var_1d).unwrap_or(356.11)
    } else {
        356.11 // Mock value
    };
    
    let response = PortfolioResponse {
        total_value: valuation_result.total_value,
        total_pnl: positions.iter().map(|p| p.pnl).sum(),
        total_var: var_1d,
        portfolio_volatility: valuation_result.risk_metrics.as_ref().and_then(|r| r.volatility).unwrap_or(20.0),
        sharpe_ratio: 1.25, // Mock value
        max_drawdown: 8.5,  // Mock value
        positions,
        greeks: GreeksResponse {
            total_delta,
            total_gamma,
            total_theta,
            total_vega,
            total_rho,
        },
        exposures: ExposuresResponse {
            by_instrument_type,
            by_underlying,
        },
    };
    
    Ok(Json(response))
}

async fn portfolio_stream() -> Response {
    let stream = IntervalStream::new(interval(TokioDuration::from_secs(5)))
        .then(|_| async {
            match generate_portfolio_data().await {
                Ok(data) => {
                    let json = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
                    Ok::<String, std::io::Error>(format!("data: {}\n\n", json))
                }
                Err(_) => Ok("data: {}\n\n".to_string())
            }
        });

    let body = axum::body::Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap()
}

async fn generate_portfolio_data() -> Result<PortfolioResponse, StatusCode> {
    // Initialize services
    let market_data = MockMarketDataProvider::new();
    let black_scholes = BlackScholesModel::new();
    let risk_engine = RiskEngine::new(0.95, 1, 10000);
    let portfolio_service = PortfolioValuationService::new(risk_engine);
    
    // Create sample instruments
    let aapl_stock = Stock::new("AAPL".to_string(), "USD".to_string(), 100.0);
    let aapl_call = FinancialOption::new(
        "AAPL".to_string(),
        "USD".to_string(),
        OptionType::Call,
        180.0,
        Utc::now() + Duration::days(30),
        10.0,
        ExerciseStyle::European,
    );
    
    // Create portfolio
    let mut portfolio = Portfolio::new("Demo Portfolio".to_string(), "USD".to_string());
    portfolio.add_position(aapl_stock.id().to_string(), 100.0, Some(175.00));
    portfolio.add_position(aapl_call.id().to_string(), 10.0, Some(5.50));
    
    // Prepare instruments map
    let mut instruments = HashMap::new();
    instruments.insert(aapl_stock.id().to_string(), Box::new(aapl_stock) as Box<dyn Instrument + Send + Sync>);
    instruments.insert(aapl_call.id().to_string(), Box::new(aapl_call) as Box<dyn Instrument + Send + Sync>);
    
    // Get market context
    let market_context = market_data.get_market_context("AAPL").await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Value portfolio
    let valuation_result = portfolio_service.value_portfolio(&portfolio, &instruments, &black_scholes, &market_context).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Calculate Greeks for each position
    let mut positions = Vec::new();
    let mut total_delta = 0.0;
    let mut total_gamma = 0.0;
    let mut total_theta = 0.0;
    let mut total_vega = 0.0;
    let mut total_rho = 0.0;
    
    for position in portfolio.positions.iter() {
        if let Some(instrument) = instruments.get(&position.instrument_id) {
            let position_value = valuation_result.positions.iter()
                .find(|p| p.instrument_id == position.instrument_id)
                .map(|p| p.total_value)
                .unwrap_or(0.0);
            let cost_basis = position.average_cost.unwrap_or(0.0) * position.quantity;
            let pnl = position_value - cost_basis;
            let weight = (position_value / valuation_result.total_value) * 100.0;
            
            // Calculate Greeks
            let greeks = black_scholes.calculate_greeks(instrument.as_ref(), &market_context)
                .unwrap_or_else(|_| valuation_service::valuation::Greeks {
                    delta: Some(0.0),
                    gamma: Some(0.0),
                    theta: Some(0.0),
                    vega: Some(0.0),
                    rho: Some(0.0),
                });
            
            let pos_delta = greeks.delta.unwrap_or(0.0) * position.quantity;
            let pos_gamma = greeks.gamma.unwrap_or(0.0) * position.quantity;
            let pos_theta = greeks.theta.unwrap_or(0.0) * position.quantity;
            let pos_vega = greeks.vega.unwrap_or(0.0) * position.quantity;
            let pos_rho = greeks.rho.unwrap_or(0.0) * position.quantity;
            
            total_delta += pos_delta;
            total_gamma += pos_gamma;
            total_theta += pos_theta;
            total_vega += pos_vega;
            total_rho += pos_rho;
            
            let instrument_type = match instrument.instrument_type() {
                valuation_service::valuation::InstrumentType::Stock => "Stock",
                valuation_service::valuation::InstrumentType::Option => "Option",
                _ => "Other",
            };
            
            let symbol = if instrument_type == "Stock" {
                "AAPL".to_string()
            } else {
                "AAPL Call $180".to_string()
            };
            
            positions.push(PositionResponse {
                instrument_id: position.instrument_id.clone(),
                instrument_type: instrument_type.to_string(),
                symbol,
                quantity: position.quantity,
                market_value: position_value,
                pnl,
                weight,
                delta: pos_delta,
                gamma: pos_gamma,
                theta: pos_theta,
                vega: pos_vega,
                rho: pos_rho,
            });
        }
    }
    
    // Calculate exposures
    let mut by_instrument_type = HashMap::new();
    let mut by_underlying = HashMap::new();
    
    for position in &positions {
        *by_instrument_type.entry(position.instrument_type.clone()).or_insert(0.0) += position.market_value;
        *by_underlying.entry("AAPL".to_string()).or_insert(0.0) += position.market_value;
    }
    
    // Calculate risk metrics
    let var_1d = valuation_result.risk_metrics.as_ref().and_then(|r| r.var_1d).unwrap_or(356.11);
    
    let response = PortfolioResponse {
        total_value: valuation_result.total_value,
        total_pnl: positions.iter().map(|p| p.pnl).sum(),
        total_var: var_1d,
        portfolio_volatility: valuation_result.risk_metrics.as_ref().and_then(|r| r.volatility).unwrap_or(20.0),
        sharpe_ratio: 1.25, // Mock value
        max_drawdown: 8.5,  // Mock value
        positions,
        greeks: GreeksResponse {
            total_delta,
            total_gamma,
            total_theta,
            total_vega,
            total_rho,
        },
        exposures: ExposuresResponse {
            by_instrument_type,
            by_underlying,
        },
    };
    
    Ok(response)
}

async fn health_check() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    
    let app = Router::new()
        .route("/api/portfolio", get(get_portfolio_data))
        .route("/api/portfolio/stream", get(portfolio_stream))
        .route("/health", get(health_check))
        .layer(ServiceBuilder::new().layer(cors));
    
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    println!("🚀 Valuation Service API running on http://localhost:8080");
    println!("📊 Dashboard API available at http://localhost:8080/api/portfolio");
    
    axum::serve(listener, app).await.unwrap();
}
