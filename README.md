# 📊 Valuation Service

A high-performance, real-time portfolio valuation service built with Rust, Axum, and Tokio. This service provides RESTful APIs for portfolio management and real-time updates via Server-Sent Events (SSE).

## ✨ Features

- **Real-time Updates**: Live portfolio valuation updates using Server-Sent Events (SSE)
- **RESTful API**: Simple HTTP endpoints for portfolio management
- **High Performance**: Built on Axum and Tokio for maximum throughput and low latency
- **CORS Ready**: Pre-configured for seamless web frontend integration

## ⚡ Quickstart (PostgreSQL + backend)

From the root of this service repository (`valuation-service/`), run:

```bash
./start-dev.sh
```

The script is checked into this repository and is location‑independent (it resolves paths relative to the script file).

Defaults:
- Uses `DATABASE_URL=postgres://postgres:postgres@localhost:5433/valuation`
- Backend: `valuation-service/` on `http://localhost:3000`
  (Frontend should be started separately.)

Make sure the script is executable:

```bash
chmod +x start-dev.sh
```

To override the database URL:

```bash
DATABASE_URL="postgres://postgres:postgres@localhost:5432/valuation" ./start-dev.sh
```

## 🚀 Getting Started

### Prerequisites

- PostgreSQL 13+ server (local or Docker), reachable via `DATABASE_URL`
  - Default used by the dev script: `postgres://postgres:postgres@localhost:5433/valuation`
  - Alternatively set your own `DATABASE_URL`
- Rust 1.65+ (install via [rustup](https://rustup.rs/))
- Cargo (Rust's package manager)

If you don’t use Docker, install Postgres locally (WSL example):

```bash
sudo apt-get update
sudo apt-get install -y postgresql postgresql-contrib
sudo service postgresql start
sudo -u postgres psql -c "ALTER USER postgres WITH PASSWORD 'postgres';"
sudo -u postgres createdb valuation

# If needed, enable password auth and localhost access (edit pg_hba.conf)
# Ensure Postgres is listening (commonly on port 5433 with PGDG packages)
```

### Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/valuation-service.git
cd valuation-service

# Build in release mode
cargo build --release
```

## ▶️ Running the service

Preferred (starts PostgreSQL if available and the backend):

```bash
./valuation-service/start-dev.sh
```

Manual run (PostgreSQL must already be running; this does NOT start it):

Set `DATABASE_URL` (example if Postgres runs on 5433):

```bash
export DATABASE_URL="postgres://postgres:postgres@localhost:5433/valuation"
cargo run --release
```

If PostgreSQL is not running, you can start it (Ubuntu/WSL):

```bash
sudo service postgresql start
```

## 📚 API Reference

### Base URL
All API endpoints are relative to `http://localhost:3000`.

### Authentication
No authentication is required.

### Notes
- The portfolio now starts empty (no mock data).
- Some endpoints return mock-style responses but do not yet persist changes.

### Endpoints

#### 1) Portfolio

##### Get Portfolio
```http
GET /portfolio
```

Returns the current portfolio snapshot.

Example
```bash
curl -s http://localhost:3000/portfolio | jq
```

Response
```json
{
  "timestamp": "2025-08-18T09:45:00Z",
  "portfolio_value": 0.0,
  "positions": []
}
```

#### 2) Positions

Endpoints to add, update, and delete positions. These currently emit an update event but do not persist state yet.

##### Add Position
```http
POST /portfolio/positions
```

Request Body
```json
{
  "symbol": "string",
  "quantity": 123.45,
  "average_cost": 100.0
}
```

Example
```bash
curl -X POST http://localhost:3000/portfolio/positions \
  -H "Content-Type: application/json" \
  -d '{"symbol":"GOOGL","quantity":5,"average_cost":2800.0}'
```

Response
```json
{
  "position_id": "550e8400-e29b-41d4-a716-446655440000",
  "symbol": "GOOGL",
  "quantity": 5.0,
  "average_cost": 2800.0,
  "status": "added"
}
```

##### Update Position
```http
PUT /portfolio/positions/{position_id}
```

Request Body
```json
{ "quantity": 8.0 }
```

Example
```bash
curl -X PUT http://localhost:3000/portfolio/positions/550e8400-e29b-41d4-a716-446655440000 \
  -H "Content-Type: application/json" \
  -d '{"quantity": 8}'
```

Response
```json
{ "position_id": "550e8400-e29b-41d4-a716-446655440000", "quantity": 8.0, "status": "updated" }
```

##### Delete Position
```http
DELETE /portfolio/positions/{position_id}
```

Example
```bash
curl -X DELETE http://localhost:3000/portfolio/positions/550e8400-e29b-41d4-a716-446655440000
```

Response
```json
{ "position_id": "550e8400-e29b-41d4-a716-446655440000", "status": "deleted" }
```

#### 3) Market Data

##### Update Stock Price
```http
POST /update-price
```

Request Body
```json
{ "symbol": "AAPL", "price": 190.50 }
```

Example
```bash
curl -X POST http://localhost:3000/update-price \
  -H "Content-Type: application/json" \
  -d '{"symbol":"AAPL","price":190.50}'
```

Response
```
200 OK
```

#### 4) Analysis

##### Get Portfolio Risk Metrics
```http
GET /portfolio/analysis/risk
```

Example
```bash
curl -s http://localhost:3000/portfolio/analysis/risk | jq
```

Response (mocked)
```json
{
  "portfolio_value": 1000000.0,
  "value_at_risk_1d_95": 25000.0,
  "value_at_risk_10d_95": 75000.0,
  "expected_shortfall_95": 35000.0,
  "volatility_1y": 0.2,
  "beta": 1.05,
  "sharpe_ratio": 1.2,
  "sortino_ratio": 1.5,
  "max_drawdown": 0.15,
  "last_updated": "2025-08-17T23:30:45.123Z"
}
```

##### Get Portfolio Performance Metrics
```http
GET /portfolio/analysis/performance
```

Example
```bash
curl -s http://localhost:3000/portfolio/analysis/performance | jq
```

Response (mocked)
```json
{
  "total_return": 150000.0,
  "total_return_percentage": 15.0,
  "annualized_return": 0.18,
  "ytd_return": 0.12,
  "monthly_returns": [0.02, 0.015, -0.01, 0.03, 0.01],
  "sharpe_ratio": 1.2,
  "sortino_ratio": 1.5,
  "alpha": 0.02,
  "beta": 1.05,
  "r_squared": 0.95,
  "tracking_error": 0.08,
  "information_ratio": 0.25,
  "max_drawdown": 0.15,
  "calmar_ratio": 1.2,
  "start_date": "2024-01-01T00:00:00Z",
  "end_date": "2025-08-17T23:30:45.123Z"
}
```

#### 5) Real-time Updates

##### SSE Stream
```http
GET /stream
```

Example
```bash
curl -N http://localhost:3000/stream
```

Headers
```
Accept: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
```

Events are JSON-serialized portfolio snapshots.

#### 6) System

##### Health Check
```http
GET /health
```

Example
```bash
curl -s http://localhost:3000/health
```

Response
```
200 OK
```

## 🏗️ Architecture

The service follows a clean, layered architecture:

```mermaid
flowchart LR
    A[Client] -->|HTTP/SSE| B[API Layer]
    B --> C[Application Services]
    C --> D[Domain Models]
    D --> E[External Services]
    
    subgraph API Layer
    A1[HTTP Server]
    A2[SSE Stream]
    end
    
    subgraph Application
    B1[Portfolio Service]
    end
    
    subgraph Domain
    C1[Portfolio]
    C2[Position]
    end
    
    subgraph Infrastructure
    D1[Market Data]
    end
```

### Core Components

1. **API Layer**
   - Handles HTTP/SSE communication
   - Request/response processing
   
2. **Application Layer**
   - Business logic implementation
   - Coordinates between domain and infrastructure
   
3. **Domain Layer**
   - Core business entities
   - Business rules and validations

## 🛠 Development

### Prerequisites

- Rust 1.65+
- Cargo

### Building

```bash
# Debug build
cargo build

# Release build (recommended for production)
cargo build --release
```

### Running Locally

```bash
# Start the server
cargo run --release

# Server will be available at http://localhost:3000
```

### Environment Variables

Configure the service using `.env` file:

```env
PORT=3000
RUST_LOG=info
ENVIRONMENT=development
```

## 🧪 Testing

Run the test suite:

```bash
# Run all tests
cargo test

# Run tests with detailed output
cargo test -- --nocapture

# Run a specific test
cargo test test_portfolio_endpoint -- --nocapture
```

## 🚀 Deployment

### Docker

Build and run using Docker:

```bash
docker build -t valuation-service .
docker run -p 3000:3000 --env-file .env valuation-service
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Code Style

- Follow Rust's official style guide
- Run `cargo fmt` before committing
- Run `cargo clippy` to catch common mistakes

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built with ❤️ using [Rust](https://www.rust-lang.org/)
- Powered by [Axum](https://github.com/tokio-rs/axum) and [Tokio](https://tokio.rs/)
