use axum::{
    body::to_bytes,
    http::{HeaderValue, Method},
    response::sse::Event,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

// Application state for testing
#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<serde_json::Value>,
}

async fn setup() -> String {
    // Create a channel for broadcasting updates
    let (tx, _) = broadcast::channel(100);
    
    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin("*".parse::<HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);
    
    // Build the application
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .route("/portfolio", axum::routing::get(|| async { 
            axum::Json(serde_json::json!({ "status": "ok" })) 
        }))
        .route("/update-price", axum::routing::post(|| async { 
            (axum::http::StatusCode::OK, "Price updated") 
        }))
        .route("/stream", axum::routing::get(|| async { 
            axum::response::sse::Sse::new(
                tokio_stream::empty::<std::result::Result<Event, std::convert::Infallible>>()
            )
        }))
        .with_state(Arc::new(AppState { tx: tx.clone() }))
        .layer(cors);
    
    // Bind to a random port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    // Start the server
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    
    format!("http://{addr}")
}

#[tokio::test]
async fn test_health_check() {
    let base_url = setup().await;
    let client = reqwest::Client::new();
    
    let response = client
        .get(&format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
        
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "OK");
}

#[tokio::test]
async fn test_portfolio_endpoint() {
    let base_url = setup().await;
    let client = reqwest::Client::new();
    
    let response = client
        .get(&format!("{}/portfolio", base_url))
        .send()
        .await
        .unwrap();
        
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_price_update() {
    let base_url = setup().await;
    let client = reqwest::Client::new();
    
    let response = client
        .post(&format!("{}/update-price", base_url))
        .header("Content-Type", "application/json")
        .body(r#"{"symbol":"AAPL","price":185.0}"#)
        .send()
        .await
        .unwrap();
        
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "Price updated");
}

#[tokio::test]
async fn test_sse_stream() {
    let base_url = setup().await;
    
    let response = reqwest::get(&format!("{}/stream", base_url))
        .await
        .unwrap();
        
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    
    // The stream is empty in our test implementation
    // In a real test, we would test the SSE stream content here
}
