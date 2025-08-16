use chrono::{Duration, Utc};
use valuation_service::{
    instruments::{FinancialOption, OptionType, ExerciseStyle, Stock},
    market_data::{MockMarketDataProvider, MarketDataProvider},
    models::BlackScholesModel,
    portfolio::{Portfolio, PortfolioValuationService},
    risk::RiskEngine,
    valuation::Instrument,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Valuation Service Demo");
    
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
    
    println!("📊 Created instruments:");
    println!("  - Stock: {} shares of AAPL", aapl_stock.notional());
    println!("  - Option: {} AAPL call options, strike ${}", aapl_call.notional(), 180.0);
    
    // Create portfolio
    let mut portfolio = Portfolio::new("Demo Portfolio".to_string(), "USD".to_string());
    portfolio.add_position(aapl_stock.id().to_string(), 100.0, Some(175.00));
    portfolio.add_position(aapl_call.id().to_string(), 10.0, Some(5.50));
    
    println!("💼 Created portfolio with {} positions", portfolio.get_total_positions());
    
    // Prepare instruments map
    let mut instruments = std::collections::HashMap::new();
    instruments.insert(aapl_stock.id().to_string(), Box::new(aapl_stock) as Box<dyn Instrument + Send + Sync>);
    instruments.insert(aapl_call.id().to_string(), Box::new(aapl_call) as Box<dyn Instrument + Send + Sync>);
    
    // Get market context
    let market_context = market_data.get_market_context("AAPL").await?;
    println!("📈 Market context: AAPL @ ${:.2}, vol: {:.1}%, rate: {:.2}%", 
             market_context.spot_price.unwrap_or(0.0),
             market_context.volatility.unwrap_or(0.0) * 100.0,
             market_context.risk_free_rate * 100.0);
    
    // Value portfolio
    let valuation = portfolio_service.value_portfolio(
        &portfolio,
        &instruments,
        &black_scholes,
        &market_context,
    ).await?;
    
    println!("\n💰 Portfolio Valuation Results:");
    println!("  Total Value: ${:.2} {}", valuation.total_value, valuation.currency);
    println!("  Positions:");
    
    for position in &valuation.positions {
        println!("    - {}: {} units @ ${:.2} = ${:.2} ({:.1}%)",
                 position.instrument_id[..8].to_string() + "...",
                 position.quantity,
                 position.unit_value,
                 position.total_value,
                 position.weight);
        
        if let Some(pnl) = position.pnl {
            let pnl_pct = position.pnl_percentage.unwrap_or(0.0);
            println!("      P&L: ${:.2} ({:.1}%)", pnl, pnl_pct);
        }
        
        if let Some(greeks) = &position.valuation_result.greeks {
            if let Some(delta) = greeks.delta {
                println!("      Delta: {:.3}", delta);
            }
        }
    }
    
    if let Some(performance) = &valuation.performance {
        println!("\n📊 Performance Metrics:");
        println!("  Total Return: ${:.2} ({:.2}%)", 
                 performance.total_return, 
                 performance.total_return_percentage);
    }
    
    if let Some(risk_metrics) = &valuation.risk_metrics {
        println!("\n⚠️  Risk Metrics:");
        if let Some(var_1d) = risk_metrics.var_1d {
            println!("  1-Day VaR (95%): ${:.2}", var_1d);
        }
        if let Some(vol) = risk_metrics.volatility {
            println!("  Portfolio Volatility: {:.1}%", vol * 100.0);
        }
    }
    
    println!("\n✅ Valuation Service Demo completed successfully!");
    Ok(())
}
