use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{sse::Event, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
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

// Handler for GET /portfolio
async fn get_portfolio(_state: State<Arc<AppState>>) -> impl IntoResponse {
    let update = generate_portfolio_update().await;
    Json(update)
}

// Handler for POST /update-price
async fn update_price(
    state: State<Arc<AppState>>,
    Json(_update): Json<UpdatePriceRequest>,
) -> impl IntoResponse {
    let update = generate_portfolio_update().await;
    let _ = state.tx.send(update.clone());
    (StatusCode::OK, Json(update))
}

// Handler for GET /stream
async fn stream_updates(State(state): State<Arc<AppState>>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = async_stream::stream! {
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
        .route("/health", get(health_check))
        .route("/portfolio", get(get_portfolio))
        .route("/update-price", post(update_price))
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
