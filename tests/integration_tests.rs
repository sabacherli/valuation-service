use chrono::{Duration, Utc};
use valuation_service::{
    instruments::{FinancialOption, OptionType, ExerciseStyle, Stock},
    market_data::{MockMarketDataProvider, MarketDataProvider},
    models::{BlackScholesModel, MonteCarloModel},
    portfolio::{Portfolio, PortfolioValuationService},
    risk::RiskEngine,
    valuation::{Instrument, Valuator},
};

#[tokio::test]
async fn test_black_scholes_option_valuation() {
    let option = FinancialOption::new(
        "AAPL".to_string(),
        "USD".to_string(),
        OptionType::Call,
        180.0,
        Utc::now() + Duration::days(30),
        1.0,
        ExerciseStyle::European,
    );

    let market_data = MockMarketDataProvider::new();
    let context = market_data.get_market_context("AAPL").await.unwrap();
    
    let model = BlackScholesModel::new();
    let result = model.value(&option, &context).unwrap();
    
    assert!(result.value > 0.0);
    assert_eq!(result.currency, "USD");
    assert!(result.confidence > 0.0);
    assert!(result.greeks.is_some());
}

#[tokio::test]
async fn test_monte_carlo_option_valuation() {
    let option = FinancialOption::new(
        "AAPL".to_string(),
        "USD".to_string(),
        OptionType::Put,
        170.0,
        Utc::now() + Duration::days(60),
        1.0,
        ExerciseStyle::European,
    );

    let market_data = MockMarketDataProvider::new();
    let context = market_data.get_market_context("AAPL").await.unwrap();
    
    let model = MonteCarloModel::new(10000, 252);
    let result = model.value(&option, &context).unwrap();
    
    assert!(result.value >= 0.0);
    assert_eq!(result.currency, "USD");
    assert!(result.confidence > 0.0);
}

#[tokio::test]
async fn test_portfolio_valuation() {
    let mut portfolio = Portfolio::new("Test Portfolio".to_string(), "USD".to_string());
    
    let stock = Stock::new("AAPL".to_string(), "USD".to_string(), 100.0);
    let option = FinancialOption::new(
        "AAPL".to_string(),
        "USD".to_string(),
        OptionType::Call,
        180.0,
        Utc::now() + Duration::days(30),
        10.0,
        ExerciseStyle::European,
    );
    
    portfolio.add_position(stock.id().to_string(), 100.0, Some(175.00));
    portfolio.add_position(option.id().to_string(), 10.0, Some(5.50));
    
    let mut instruments = std::collections::HashMap::new();
    instruments.insert(stock.id().to_string(), Box::new(stock) as Box<dyn Instrument + Send + Sync>);
    instruments.insert(option.id().to_string(), Box::new(option) as Box<dyn Instrument + Send + Sync>);
    
    let market_data = MockMarketDataProvider::new();
    let context = market_data.get_market_context("AAPL").await.unwrap();
    
    let risk_engine = RiskEngine::new(0.95, 1, 1000);
    let portfolio_service = PortfolioValuationService::new(risk_engine);
    let model = BlackScholesModel::new();
    
    let valuation = portfolio_service.value_portfolio(
        &portfolio,
        &instruments,
        &model,
        &context,
    ).await.unwrap();
    
    assert!(valuation.total_value > 0.0);
    assert_eq!(valuation.positions.len(), 2);
    assert_eq!(valuation.currency, "USD");
}

#[tokio::test]
async fn test_risk_metrics_calculation() {
    let risk_engine = RiskEngine::new(0.95, 1, 10000);
    
    let returns = vec![-0.05, -0.03, -0.01, 0.01, 0.02, 0.03, 0.04, -0.02, -0.04, 0.01];
    
    let var = risk_engine.calculate_var(&returns).unwrap();
    let es = risk_engine.calculate_expected_shortfall(&returns).unwrap();
    let vol = risk_engine.calculate_volatility(&returns).unwrap();
    
    assert!(var > 0.0);
    assert!(es >= var);
    assert!(vol > 0.0);
}

#[tokio::test]
async fn test_greeks_calculation() {
    let option = FinancialOption::new(
        "AAPL".to_string(),
        "USD".to_string(),
        OptionType::Call,
        175.0,
        Utc::now() + Duration::days(30),
        1.0,
        ExerciseStyle::European,
    );

    let market_data = MockMarketDataProvider::new();
    let context = market_data.get_market_context("AAPL").await.unwrap();
    
    let model = BlackScholesModel::new();
    let greeks = model.calculate_greeks(&option, &context).unwrap();
    
    assert!(greeks.delta.is_some());
    assert!(greeks.gamma.is_some());
    assert!(greeks.theta.is_some());
    assert!(greeks.vega.is_some());
    assert!(greeks.rho.is_some());
    
    // Delta should be between 0 and 1 for call options
    let delta = greeks.delta.unwrap();
    assert!(delta >= 0.0 && delta <= 1.0);
}

#[test]
fn test_portfolio_operations() {
    let mut portfolio = Portfolio::new("Test".to_string(), "USD".to_string());
    
    let position_id = portfolio.add_position("AAPL".to_string(), 100.0, Some(175.00));
    assert_eq!(portfolio.get_total_positions(), 1);
    
    portfolio.update_position(&position_id, 150.0).unwrap();
    let position = portfolio.positions.iter().find(|p| p.id == position_id).unwrap();
    assert_eq!(position.quantity, 150.0);
    
    portfolio.remove_position(&position_id).unwrap();
    assert_eq!(portfolio.get_total_positions(), 0);
}
