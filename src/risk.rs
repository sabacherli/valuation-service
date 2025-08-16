use crate::{Result, ValuationError, RiskMetrics};
use nalgebra as na;
use rand::prelude::*;
use rand_distr::StandardNormal;
use statrs::distribution::{ContinuousCDF, Normal};

pub struct RiskEngine {
    confidence_level: f64,
    time_horizon_days: i64,
    num_simulations: usize,
}

impl RiskEngine {
    pub fn new(confidence_level: f64, time_horizon_days: i64, num_simulations: usize) -> Self {
        Self {
            confidence_level,
            time_horizon_days,
            num_simulations,
        }
    }

    pub fn calculate_var(&self, returns: &[f64]) -> Result<f64> {
        if returns.is_empty() {
            return Err(ValuationError::RiskCalculation("Empty returns vector".to_string()));
        }

        let mut sorted_returns = returns.to_vec();
        sorted_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let index = ((1.0 - self.confidence_level) * returns.len() as f64) as usize;
        let var = -sorted_returns[index.min(sorted_returns.len() - 1)];
        
        Ok(var)
    }

    pub fn calculate_expected_shortfall(&self, returns: &[f64]) -> Result<f64> {
        if returns.is_empty() {
            return Err(ValuationError::RiskCalculation("Empty returns vector".to_string()));
        }

        let mut sorted_returns = returns.to_vec();
        sorted_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let cutoff_index = ((1.0 - self.confidence_level) * returns.len() as f64) as usize;
        let tail_returns: Vec<f64> = sorted_returns.iter().take(cutoff_index + 1).cloned().collect();
        
        if tail_returns.is_empty() {
            return Ok(0.0);
        }
        
        let es = -tail_returns.iter().sum::<f64>() / tail_returns.len() as f64;
        Ok(es)
    }

    pub fn calculate_volatility(&self, returns: &[f64]) -> Result<f64> {
        if returns.len() < 2 {
            return Err(ValuationError::RiskCalculation("Insufficient data for volatility calculation".to_string()));
        }

        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter()
            .map(|&x| (x - mean).powi(2))
            .sum::<f64>() / (returns.len() - 1) as f64;
        
        Ok(variance.sqrt())
    }

    pub fn simulate_portfolio_returns(
        &self,
        portfolio_value: f64,
        volatility: f64,
        drift: f64,
    ) -> Result<Vec<f64>> {
        let mut rng = thread_rng();
        let dt: f64 = 1.0 / 252.0; // Daily time step
        let sqrt_dt = dt.sqrt();
        
        let returns: Vec<f64> = (0..self.num_simulations)
            .map(|_| {
                let mut value = portfolio_value;
                for _ in 0..self.time_horizon_days {
                    let z: f64 = rng.sample(StandardNormal);
                    let return_rate = drift * dt + volatility * sqrt_dt * z;
                    value *= 1.0 + return_rate;
                }
                (value - portfolio_value) / portfolio_value
            })
            .collect();
        
        Ok(returns)
    }

    pub fn calculate_portfolio_risk_metrics(
        &self,
        portfolio_value: f64,
        volatility: f64,
        drift: f64,
    ) -> Result<RiskMetrics> {
        let returns = self.simulate_portfolio_returns(portfolio_value, volatility, drift)?;
        
        let var_1d = if self.time_horizon_days >= 1 {
            Some(self.calculate_var(&returns)? * portfolio_value)
        } else {
            None
        };
        
        let var_10d = if self.time_horizon_days >= 10 {
            let scaled_volatility = volatility * (10.0_f64).sqrt();
            let returns_10d = self.simulate_portfolio_returns(portfolio_value, scaled_volatility, drift * 10.0)?;
            Some(self.calculate_var(&returns_10d)? * portfolio_value)
        } else {
            None
        };
        
        let expected_shortfall = Some(self.calculate_expected_shortfall(&returns)? * portfolio_value);
        
        Ok(RiskMetrics {
            var_1d,
            var_10d,
            expected_shortfall,
            volatility: Some(volatility),
        })
    }

    pub fn calculate_correlation_matrix(&self, returns_matrix: &[Vec<f64>]) -> Result<na::DMatrix<f64>> {
        if returns_matrix.is_empty() {
            return Err(ValuationError::RiskCalculation("Empty returns matrix".to_string()));
        }

        let n_assets = returns_matrix.len();
        let n_observations = returns_matrix[0].len();
        
        // Check all assets have same number of observations
        for returns in returns_matrix {
            if returns.len() != n_observations {
                return Err(ValuationError::RiskCalculation("Inconsistent number of observations".to_string()));
            }
        }

        let mut correlation_matrix = na::DMatrix::zeros(n_assets, n_assets);
        
        // Calculate means
        let means: Vec<f64> = returns_matrix.iter()
            .map(|returns| returns.iter().sum::<f64>() / returns.len() as f64)
            .collect();

        // Calculate correlation coefficients
        for i in 0..n_assets {
            for j in 0..n_assets {
                if i == j {
                    correlation_matrix[(i, j)] = 1.0;
                } else {
                    let numerator: f64 = (0..n_observations)
                        .map(|k| (returns_matrix[i][k] - means[i]) * (returns_matrix[j][k] - means[j]))
                        .sum();
                    
                    let var_i: f64 = returns_matrix[i].iter()
                        .map(|&x| (x - means[i]).powi(2))
                        .sum::<f64>();
                    
                    let var_j: f64 = returns_matrix[j].iter()
                        .map(|&x| (x - means[j]).powi(2))
                        .sum::<f64>();
                    
                    let denominator = (var_i * var_j).sqrt();
                    
                    if denominator > 0.0 {
                        correlation_matrix[(i, j)] = numerator / denominator;
                    } else {
                        correlation_matrix[(i, j)] = 0.0;
                    }
                }
            }
        }
        
        Ok(correlation_matrix)
    }

    pub fn calculate_portfolio_var(
        &self,
        weights: &[f64],
        volatilities: &[f64],
        correlation_matrix: &na::DMatrix<f64>,
        portfolio_value: f64,
    ) -> Result<f64> {
        if weights.len() != volatilities.len() || weights.len() != correlation_matrix.nrows() {
            return Err(ValuationError::RiskCalculation("Dimension mismatch in portfolio VaR calculation".to_string()));
        }

        let n = weights.len();
        let mut portfolio_variance = 0.0;
        
        for i in 0..n {
            for j in 0..n {
                portfolio_variance += weights[i] * weights[j] * volatilities[i] * volatilities[j] * correlation_matrix[(i, j)];
            }
        }
        
        let portfolio_volatility = portfolio_variance.sqrt();
        let normal = Normal::new(0.0, 1.0).map_err(|e| ValuationError::RiskCalculation(e.to_string()))?;
        let z_score = normal.inverse_cdf(1.0 - self.confidence_level);
        
        let var = portfolio_value * portfolio_volatility * z_score * (self.time_horizon_days as f64 / 252.0).sqrt();
        
        Ok(var)
    }

    pub fn calculate_component_var(
        &self,
        weights: &[f64],
        volatilities: &[f64],
        correlation_matrix: &na::DMatrix<f64>,
        portfolio_value: f64,
    ) -> Result<Vec<f64>> {
        let portfolio_var = self.calculate_portfolio_var(weights, volatilities, correlation_matrix, portfolio_value)?;
        let n = weights.len();
        let mut component_vars = Vec::with_capacity(n);
        
        // Calculate marginal VaR for each asset
        for i in 0..n {
            let mut marginal_var = 0.0;
            for j in 0..n {
                marginal_var += weights[j] * volatilities[j] * correlation_matrix[(i, j)];
            }
            marginal_var *= volatilities[i];
            
            let component_var = (weights[i] * portfolio_value * marginal_var / portfolio_var.abs()) * portfolio_var;
            component_vars.push(component_var);
        }
        
        Ok(component_vars)
    }

    pub fn stress_test(&self, base_value: f64, stress_scenarios: &[StressScenario]) -> Result<Vec<StressTestResult>> {
        let mut results = Vec::new();
        
        for scenario in stress_scenarios {
            let stressed_value = match scenario.scenario_type {
                StressType::MarketShock => {
                    base_value * (1.0 + scenario.shock_magnitude)
                }
                StressType::VolatilityShock => {
                    // Simplified volatility stress - in practice would be more complex
                    let vol_impact = scenario.shock_magnitude * 0.1; // 10% of shock affects value
                    base_value * (1.0 - vol_impact.abs())
                }
                StressType::RateShock => {
                    // Simplified rate shock - duration would be needed for bonds
                    let rate_impact = scenario.shock_magnitude * 0.05; // 5% of shock affects value
                    base_value * (1.0 - rate_impact)
                }
            };
            
            results.push(StressTestResult {
                scenario_name: scenario.name.clone(),
                base_value,
                stressed_value,
                pnl: stressed_value - base_value,
                pnl_percentage: (stressed_value - base_value) / base_value * 100.0,
            });
        }
        
        Ok(results)
    }
}

#[derive(Debug, Clone)]
pub struct StressScenario {
    pub name: String,
    pub scenario_type: StressType,
    pub shock_magnitude: f64, // As a percentage (e.g., -0.20 for -20%)
}

#[derive(Debug, Clone)]
pub enum StressType {
    MarketShock,
    VolatilityShock,
    RateShock,
}

#[derive(Debug, Clone)]
pub struct StressTestResult {
    pub scenario_name: String,
    pub base_value: f64,
    pub stressed_value: f64,
    pub pnl: f64,
    pub pnl_percentage: f64,
}

impl Default for RiskEngine {
    fn default() -> Self {
        Self::new(0.95, 1, 10000)
    }
}
