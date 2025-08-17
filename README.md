# Valuation Service

A lightweight Rust-based web service for real-time portfolio valuation with Server-Sent Events (SSE) for live updates.

## Features

- **Real-time Updates**: Server-Sent Events (SSE) for live portfolio valuation
- **RESTful API**: Simple HTTP endpoints for portfolio management
- **Modern Web Stack**: Built with Axum and Tokio for high performance
- **CORS Support**: Ready for web frontend integration

### API Endpoints

#### Portfolio Management
- `GET /portfolio` - Get current portfolio valuation
  ```bash
  curl http://localhost:3000/portfolio
  ```

- `POST /update-price` - Update a stock price
  ```bash
  curl -X POST http://localhost:3000/update-price \
    -H "Content-Type: application/json" \
    -d '{"symbol": "AAPL", "price": 185.0}'
  ```
  
  Request Body:
  ```json
  {
    "symbol": "string",  // Stock symbol (e.g., "AAPL", "MSFT")
    "price": number      // New price for the stock
  }
  ```

#### Real-time Updates
- `GET /stream` - Server-Sent Events (SSE) stream of portfolio updates
  ```bash
  curl -N http://localhost:3000/stream
  ```

#### System
- `GET /health` - Health check endpoint
  ```bash
  curl http://localhost:3000/health
  ```

## Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   REST API      │    │  Portfolio      │    │  Risk Engine    │
│   (Axum)        │    │  Service        │    │                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  Instruments    │    │  Valuators      │    │  Market Data    │
│  (Stocks,       │    │  (Black-Scholes,│    │  Providers      │
│   Options,      │    │   Monte Carlo)  │    │                 │
│   Bonds)        │    │                 │    │                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## Quick Start

### Building and Running

```bash
# Clone and build
cd valuation-service
cargo build --release

# Run the service
cargo run

# Run tests
cargo test
```

The service will start on `http://localhost:3000`

### API Endpoints

#### Health Check
```bash
curl http://localhost:3000/health
```

#### Create Portfolio
```bash
curl -X POST http://localhost:3000/portfolios \
  -H "Content-Type: application/json" \
  -d '{"name": "My Portfolio", "base_currency": "USD"}'
```

#### Create Stock Instrument
```bash
curl -X POST http://localhost:3000/instruments/stocks \
  -H "Content-Type: application/json" \
  -d '{
    "symbol": "AAPL",
    "currency": "USD",
    "shares": 100,
    "price": 175.50,
    "volatility": 0.25,
    "dividend_yield": 0.0045
  }'
```

#### Create Option Instrument
```bash
curl -X POST http://localhost:3000/instruments/options \
  -H "Content-Type: application/json" \
  -d '{
    "underlying": "AAPL",
    "currency": "USD",
    "option_type": "call",
    "strike": 180.0,
    "expiry": "2024-12-20T00:00:00Z",
    "quantity": 10,
    "exercise_style": "european"
  }'
```

#### Add Position to Portfolio
```bash
curl -X POST http://localhost:3000/portfolios/{portfolio_id}/positions \
  -H "Content-Type: application/json" \
  -d '{
    "instrument_id": "{instrument_id}",
    "quantity": 100,
    "average_cost": 175.00
  }'
```

#### Value Portfolio
```bash
# Using Black-Scholes (default)
curl "http://localhost:3000/portfolios/{portfolio_id}/valuation"

# Using Monte Carlo
curl "http://localhost:3000/portfolios/{portfolio_id}/valuation?model=monte_carlo&include_greeks=true&include_risk_metrics=true"
```

#### Get Market Data
```bash
curl http://localhost:3000/market-data/AAPL
```

## Usage Examples

### Creating a Simple Portfolio

```rust
use valuation_service::*;

// Create portfolio
let mut portfolio = Portfolio::new("Tech Portfolio".to_string(), "USD".to_string());

// Create instruments
let aapl_stock = Stock::new("AAPL".to_string(), "USD".to_string(), 100.0);
let aapl_call = Option::new(
    "AAPL".to_string(),
    "USD".to_string(),
    OptionType::Call,
    180.0,
    Utc::now() + chrono::Duration::days(30),
    10.0,
    ExerciseStyle::European,
);

// Add positions
portfolio.add_position(aapl_stock.id().to_string(), 100.0, Some(175.00));
portfolio.add_position(aapl_call.id().to_string(), 10.0, Some(5.50));
```

### Valuing with Different Models

```rust
// Black-Scholes valuation
let bs_model = BlackScholesModel::new();
let bs_result = bs_model.value(&option, &market_context)?;

// Monte Carlo valuation
let mc_model = MonteCarloModel::new(100000, 252);
let mc_result = mc_model.value(&option, &market_context)?;
```

### Risk Analysis

```rust
let risk_engine = RiskEngine::new(0.95, 1, 10000);

// Calculate VaR
let portfolio_returns = risk_engine.simulate_portfolio_returns(1000000.0, 0.20, 0.08)?;
let var_95 = risk_engine.calculate_var(&portfolio_returns)?;

// Stress testing
let stress_scenarios = vec![
    StressScenario {
        name: "Market Crash".to_string(),
        scenario_type: StressType::MarketShock,
        shock_magnitude: -0.30,
    }
];
let stress_results = risk_engine.stress_test(1000000.0, &stress_scenarios)?;
```

## Configuration

### Market Data Providers

The service supports both mock and live market data providers:

```rust
// Mock provider (for testing)
let mock_provider = MockMarketDataProvider::new();

// Live provider (requires API key)
let live_provider = LiveMarketDataProvider::new(Some("your-api-key".to_string()));
```

### Risk Engine Settings

```rust
let risk_engine = RiskEngine::new(
    0.99,    // 99% confidence level
    10,      // 10-day time horizon
    100000   // Number of Monte Carlo simulations
);
```

## Dependencies

- **serde**: Serialization/deserialization
- **tokio**: Async runtime
- **axum**: Web framework
- **chrono**: Date/time handling
- **nalgebra**: Linear algebra
- **statrs**: Statistical functions
- **rand**: Random number generation
- **reqwest**: HTTP client

## Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test module
cargo test models::tests
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass
5. Submit a pull request

## License

This project is licensed under the MIT License.

## Roadmap

### Planned Features
- **Additional Instruments**: Futures, swaps, exotic options
- **Advanced Models**: Heston, local volatility, jump-diffusion
- **Real-time Streaming**: WebSocket market data feeds
- **Database Integration**: Persistent storage for portfolios and historical data
- **Performance Optimization**: SIMD vectorization, GPU acceleration
- **Regulatory Reporting**: Basel III, FRTB compliance
- **Machine Learning**: AI-driven risk models and market predictions

### Performance Targets
- **Latency**: Sub-millisecond option pricing
- **Throughput**: 100K+ valuations per second
- **Scalability**: Horizontal scaling with microservices architecture
