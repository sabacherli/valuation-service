use crate::{Instrument, InstrumentType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stock {
    pub id: String,
    pub symbol: String,
    pub currency: String,
    pub shares: f64,
    pub sector: Option<String>,
    pub market_cap: Option<f64>,
}

impl Stock {
    pub fn new(symbol: String, currency: String, shares: f64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            symbol,
            currency,
            shares,
            sector: None,
            market_cap: None,
        }
    }
}

impl Instrument for Stock {
    fn id(&self) -> &str {
        &self.id
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Stock
    }

    fn currency(&self) -> &str {
        &self.currency
    }

    fn maturity(&self) -> std::option::Option<DateTime<Utc>> {
        None
    }

    fn notional(&self) -> f64 {
        self.shares
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bond {
    pub id: String,
    pub isin: String,
    pub currency: String,
    pub face_value: f64,
    pub coupon_rate: f64,
    pub maturity: DateTime<Utc>,
    pub issue_date: DateTime<Utc>,
    pub payment_frequency: PaymentFrequency,
    pub credit_rating: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentFrequency {
    Annual,
    SemiAnnual,
    Quarterly,
    Monthly,
}

impl Bond {
    pub fn new(
        isin: String,
        currency: String,
        face_value: f64,
        coupon_rate: f64,
        maturity: DateTime<Utc>,
        issue_date: DateTime<Utc>,
        payment_frequency: PaymentFrequency,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            isin,
            currency,
            face_value,
            coupon_rate,
            maturity,
            issue_date,
            payment_frequency,
            credit_rating: None,
        }
    }
}

impl Instrument for Bond {
    fn id(&self) -> &str {
        &self.id
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Bond
    }

    fn currency(&self) -> &str {
        &self.currency
    }

    fn maturity(&self) -> std::option::Option<DateTime<Utc>> {
        Some(self.maturity)
    }

    fn notional(&self) -> f64 {
        self.face_value
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialOption {
    pub id: String,
    pub underlying: String,
    pub currency: String,
    pub option_type: OptionType,
    pub strike: f64,
    pub expiry: DateTime<Utc>,
    pub quantity: f64,
    pub exercise_style: ExerciseStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExerciseStyle {
    European,
    American,
    Bermudan,
}

impl FinancialOption {
    pub fn new(
        underlying: String,
        currency: String,
        option_type: OptionType,
        strike: f64,
        expiry: DateTime<Utc>,
        quantity: f64,
        exercise_style: ExerciseStyle,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            underlying,
            currency,
            option_type,
            strike,
            expiry,
            quantity,
            exercise_style,
        }
    }
}

impl Instrument for FinancialOption {
    fn id(&self) -> &str {
        &self.id
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Option
    }

    fn currency(&self) -> &str {
        &self.currency
    }

    fn maturity(&self) -> std::option::Option<DateTime<Utc>> {
        Some(self.expiry)
    }

    fn notional(&self) -> f64 {
        self.quantity
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
