use thiserror::Error;

pub type Result<T> = std::result::Result<T, ValuationError>;

#[derive(Error, Debug)]
pub enum ValuationError {
    #[error("Invalid instrument data: {0}")]
    InvalidInstrument(String),
    
    #[error("Market data error: {0}")]
    MarketData(String),
    
    #[error("Pricing model error: {0}")]
    PricingModel(String),
    
    #[error("Risk calculation error: {0}")]
    RiskCalculation(String),
    
    #[error("Portfolio error: {0}")]
    Portfolio(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Date/time error: {0}")]
    DateTime(#[from] chrono::ParseError),
}
