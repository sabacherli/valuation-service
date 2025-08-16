use crate::{Greeks, Instrument, MarketContext, Result, RiskMetrics, ValuationError, ValuationResult, Valuator};
use crate::instruments::{FinancialOption, OptionType};
use chrono::Utc;
use rand::prelude::*;
use rand_distr::StandardNormal;
use statrs::distribution::{Continuous, ContinuousCDF, Normal};

pub struct BlackScholesModel;

impl BlackScholesModel {
    pub fn new() -> Self {
        Self
    }

    fn black_scholes_price(
        &self,
        spot: f64,
        strike: f64,
        time_to_expiry: f64,
        risk_free_rate: f64,
        volatility: f64,
        option_type: &OptionType,
        dividend_yield: f64,
    ) -> Result<f64> {
        if time_to_expiry <= 0.0 {
            return match option_type {
                OptionType::Call => Ok((spot - strike).max(0.0)),
                OptionType::Put => Ok((strike - spot).max(0.0)),
            };
        }

        let d1 = ((spot / strike).ln() + (risk_free_rate - dividend_yield + 0.5 * volatility.powi(2)) * time_to_expiry)
            / (volatility * time_to_expiry.sqrt());
        let d2 = d1 - volatility * time_to_expiry.sqrt();

        let normal = Normal::new(0.0, 1.0).map_err(|e| ValuationError::PricingModel(e.to_string()))?;

        let price = match option_type {
            OptionType::Call => {
                spot * (-dividend_yield * time_to_expiry).exp() * normal.cdf(d1)
                    - strike * (-risk_free_rate * time_to_expiry).exp() * normal.cdf(d2)
            }
            OptionType::Put => {
                strike * (-risk_free_rate * time_to_expiry).exp() * normal.cdf(-d2)
                    - spot * (-dividend_yield * time_to_expiry).exp() * normal.cdf(-d1)
            }
        };

        Ok(price)
    }

    fn calculate_greeks_bs(
        &self,
        spot: f64,
        strike: f64,
        time_to_expiry: f64,
        risk_free_rate: f64,
        volatility: f64,
        option_type: &OptionType,
        dividend_yield: f64,
    ) -> Result<Greeks> {
        if time_to_expiry <= 0.0 {
            return Ok(Greeks {
                delta: Some(0.0),
                gamma: Some(0.0),
                theta: Some(0.0),
                vega: Some(0.0),
                rho: Some(0.0),
            });
        }

        let d1 = ((spot / strike).ln() + (risk_free_rate - dividend_yield + 0.5 * volatility.powi(2)) * time_to_expiry)
            / (volatility * time_to_expiry.sqrt());
        let d2 = d1 - volatility * time_to_expiry.sqrt();

        let normal = Normal::new(0.0, 1.0).map_err(|e| ValuationError::PricingModel(e.to_string()))?;
        let phi_d1 = normal.pdf(d1);
        let phi_d2 = normal.pdf(d2);
        let n_d1 = normal.cdf(d1);
        let n_d2 = normal.cdf(d2);

        let delta = match option_type {
            OptionType::Call => (-dividend_yield * time_to_expiry).exp() * n_d1,
            OptionType::Put => (-dividend_yield * time_to_expiry).exp() * (n_d1 - 1.0),
        };

        let gamma = (-dividend_yield * time_to_expiry).exp() * phi_d1 / (spot * volatility * time_to_expiry.sqrt());

        let theta = match option_type {
            OptionType::Call => {
                -spot * phi_d1 * volatility * (-dividend_yield * time_to_expiry).exp() / (2.0 * time_to_expiry.sqrt())
                    - risk_free_rate * strike * (-risk_free_rate * time_to_expiry).exp() * n_d2
                    + dividend_yield * spot * (-dividend_yield * time_to_expiry).exp() * n_d1
            }
            OptionType::Put => {
                -spot * phi_d1 * volatility * (-dividend_yield * time_to_expiry).exp() / (2.0 * time_to_expiry.sqrt())
                    + risk_free_rate * strike * (-risk_free_rate * time_to_expiry).exp() * normal.cdf(-d2)
                    - dividend_yield * spot * (-dividend_yield * time_to_expiry).exp() * normal.cdf(-d1)
            }
        };

        let vega = spot * (-dividend_yield * time_to_expiry).exp() * phi_d1 * time_to_expiry.sqrt() / 100.0;

        let rho = match option_type {
            OptionType::Call => {
                strike * time_to_expiry * (-risk_free_rate * time_to_expiry).exp() * n_d2 / 100.0
            }
            OptionType::Put => {
                -strike * time_to_expiry * (-risk_free_rate * time_to_expiry).exp() * normal.cdf(-d2) / 100.0
            }
        };

        Ok(Greeks {
            delta: Some(delta),
            gamma: Some(gamma),
            theta: Some(theta / 365.0), // Convert to daily theta
            vega: Some(vega),
            rho: Some(rho),
        })
    }
}

impl Valuator for BlackScholesModel {
    fn value(&self, instrument: &dyn Instrument, context: &MarketContext) -> Result<ValuationResult> {
        let now = Utc::now();
        
        match instrument.instrument_type() {
            crate::InstrumentType::Option => {
                if let Some(opt) = instrument.as_any().downcast_ref::<FinancialOption>() {
                    let spot = context.spot_price.ok_or_else(|| 
                        ValuationError::MarketData("Missing spot price for option valuation".to_string()))?;
                    let volatility = context.volatility.ok_or_else(|| 
                        ValuationError::MarketData("Missing volatility for option valuation".to_string()))?;
                    
                    let time_to_expiry = (opt.expiry - now).num_seconds() as f64 / (365.25 * 24.0 * 3600.0);
                    let dividend_yield = context.dividend_yield.unwrap_or(0.0);
                    
                    let price = self.black_scholes_price(
                        spot,
                        opt.strike,
                        time_to_expiry,
                        context.risk_free_rate,
                        volatility,
                        &opt.option_type,
                        dividend_yield,
                    )?;
                    
                    let total_value = price * opt.quantity;
                    let greeks = self.calculate_greeks_bs(
                        spot, opt.strike, time_to_expiry, context.risk_free_rate,
                        volatility, &opt.option_type, dividend_yield
                    )?;
                    
                    Ok(ValuationResult {
                        instrument_id: instrument.id().to_string(),
                        value: total_value,
                        currency: instrument.currency().to_string(),
                        timestamp: now,
                        confidence: 0.95,
                        greeks: Some(greeks),
                        risk_metrics: None,
                    })
                } else {
                    Err(ValuationError::InvalidInstrument("Failed to downcast to FinancialOption".to_string()))
                }
            }
            crate::InstrumentType::Stock => {
                let spot = context.spot_price.ok_or_else(|| 
                    ValuationError::MarketData("Missing spot price for stock valuation".to_string()))?;
                
                let total_value = spot * instrument.notional();
                
                Ok(ValuationResult {
                    instrument_id: instrument.id().to_string(),
                    value: total_value,
                    currency: instrument.currency().to_string(),
                    timestamp: now,
                    confidence: 0.99,
                    greeks: None,
                    risk_metrics: None,
                })
            }
            _ => Err(ValuationError::PricingModel("Instrument type not supported by Black-Scholes model".to_string())),
        }
    }

    fn calculate_greeks(&self, instrument: &dyn Instrument, context: &MarketContext) -> Result<Greeks> {
        match instrument.instrument_type() {
            crate::InstrumentType::Option => {
                if let Some(opt) = instrument.as_any().downcast_ref::<FinancialOption>() {
                    let spot = context.spot_price.ok_or_else(|| 
                        ValuationError::MarketData("Missing spot price".to_string()))?;
                    let volatility = context.volatility.ok_or_else(|| 
                        ValuationError::MarketData("Missing volatility".to_string()))?;
                    
                    let time_to_expiry = (opt.expiry - Utc::now()).num_seconds() as f64 / (365.25 * 24.0 * 3600.0);
                    let dividend_yield = context.dividend_yield.unwrap_or(0.0);
                    
                    self.calculate_greeks_bs(
                        spot, opt.strike, time_to_expiry, context.risk_free_rate,
                        volatility, &opt.option_type, dividend_yield
                    )
                } else {
                    Err(ValuationError::InvalidInstrument("Failed to downcast to FinancialOption".to_string()))
                }
            }
            _ => Ok(Greeks {
                delta: None,
                gamma: None,
                theta: None,
                vega: None,
                rho: None,
            }),
        }
    }

    fn calculate_risk_metrics(&self, _instrument: &dyn Instrument, _context: &MarketContext) -> Result<RiskMetrics> {
        Ok(RiskMetrics {
            var_1d: None,
            var_10d: None,
            expected_shortfall: None,
            volatility: None,
        })
    }
}

pub struct MonteCarloModel {
    pub num_simulations: usize,
    pub time_steps: usize,
}

impl MonteCarloModel {
    pub fn new(num_simulations: usize, time_steps: usize) -> Self {
        Self {
            num_simulations,
            time_steps,
        }
    }

    fn simulate_paths(
        &self,
        spot: f64,
        risk_free_rate: f64,
        volatility: f64,
        time_to_expiry: f64,
        dividend_yield: f64,
    ) -> Vec<Vec<f64>> {
        let mut rng = thread_rng();
        let dt = time_to_expiry / self.time_steps as f64;
        let drift = risk_free_rate - dividend_yield - 0.5 * volatility.powi(2);
        let diffusion = volatility * dt.sqrt();
        
        (0..self.num_simulations)
            .map(|_| {
                let mut path = vec![spot];
                let mut current_price = spot;
                
                for _ in 0..self.time_steps {
                    let z: f64 = rng.sample(StandardNormal);
                    current_price *= (drift * dt + diffusion * z).exp();
                    path.push(current_price);
                }
                
                path
            })
            .collect()
    }
}

impl Valuator for MonteCarloModel {
    fn value(&self, instrument: &dyn Instrument, context: &MarketContext) -> Result<ValuationResult> {
        let now = Utc::now();
        
        match instrument.instrument_type() {
            crate::InstrumentType::Option => {
                if let Some(opt) = instrument.as_any().downcast_ref::<FinancialOption>() {
                    let spot = context.spot_price.ok_or_else(|| 
                        ValuationError::MarketData("Missing spot price".to_string()))?;
                    let volatility = context.volatility.ok_or_else(|| 
                        ValuationError::MarketData("Missing volatility".to_string()))?;
                    
                    let time_to_expiry = (opt.expiry - now).num_seconds() as f64 / (365.25 * 24.0 * 3600.0);
                    let dividend_yield = context.dividend_yield.unwrap_or(0.0);
                    
                    let paths = self.simulate_paths(
                        spot,
                        context.risk_free_rate,
                        volatility,
                        time_to_expiry,
                        dividend_yield,
                    );
                    
                    let payoffs: Vec<f64> = paths
                        .iter()
                        .map(|path| {
                            let final_price = path.last().unwrap();
                            match opt.option_type {
                                OptionType::Call => (final_price - opt.strike).max(0.0),
                                OptionType::Put => (opt.strike - final_price).max(0.0),
                            }
                        })
                        .collect();
                    
                    let average_payoff = payoffs.iter().sum::<f64>() / payoffs.len() as f64;
                    let discounted_value = average_payoff * (-context.risk_free_rate * time_to_expiry).exp();
                    let total_value = discounted_value * opt.quantity;
                    
                    // Calculate confidence interval
                    let variance = payoffs.iter()
                        .map(|&x| (x - average_payoff).powi(2))
                        .sum::<f64>() / (payoffs.len() - 1) as f64;
                    let std_error = (variance / payoffs.len() as f64).sqrt();
                    let confidence = if std_error > 0.0 { 
                        (1.96 * std_error / average_payoff).min(0.99).max(0.5) 
                    } else { 
                        0.95 
                    };
                    
                    Ok(ValuationResult {
                        instrument_id: instrument.id().to_string(),
                        value: total_value,
                        currency: instrument.currency().to_string(),
                        timestamp: now,
                        confidence,
                        greeks: None,
                        risk_metrics: None,
                    })
                } else {
                    Err(ValuationError::InvalidInstrument("Failed to downcast to FinancialOption".to_string()))
                }
            }
            _ => Err(ValuationError::PricingModel("Instrument type not supported by Monte Carlo model".to_string())),
        }
    }

    fn calculate_greeks(&self, _instrument: &dyn Instrument, _context: &MarketContext) -> Result<Greeks> {
        // Greeks calculation via finite differences would be implemented here
        Ok(Greeks {
            delta: None,
            gamma: None,
            theta: None,
            vega: None,
            rho: None,
        })
    }

    fn calculate_risk_metrics(&self, _instrument: &dyn Instrument, _context: &MarketContext) -> Result<RiskMetrics> {
        Ok(RiskMetrics {
            var_1d: None,
            var_10d: None,
            expected_shortfall: None,
            volatility: None,
        })
    }
}
