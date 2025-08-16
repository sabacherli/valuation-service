use crate::{Instrument, MarketContext, Result, RiskEngine, RiskMetrics, ValuationError, ValuationResult, Valuator};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    pub id: String,
    pub name: String,
    pub positions: Vec<Position>,
    pub base_currency: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub instrument_id: String,
    pub quantity: f64,
    pub average_cost: Option<f64>,
    pub entry_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioValuation {
    pub portfolio_id: String,
    pub total_value: f64,
    pub currency: String,
    pub positions: Vec<PositionValuation>,
    pub risk_metrics: Option<RiskMetrics>,
    pub timestamp: DateTime<Utc>,
    pub performance: Option<PortfolioPerformance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionValuation {
    pub position_id: String,
    pub instrument_id: String,
    pub quantity: f64,
    pub unit_value: f64,
    pub total_value: f64,
    pub weight: f64, // Percentage of portfolio
    pub pnl: Option<f64>,
    pub pnl_percentage: Option<f64>,
    pub valuation_result: ValuationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioPerformance {
    pub total_return: f64,
    pub total_return_percentage: f64,
    pub daily_return: Option<f64>,
    pub daily_return_percentage: Option<f64>,
    pub sharpe_ratio: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub volatility: Option<f64>,
}

impl Portfolio {
    pub fn new(name: String, base_currency: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            positions: Vec::new(),
            base_currency,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_position(&mut self, instrument_id: String, quantity: f64, average_cost: Option<f64>) -> String {
        let position_id = Uuid::new_v4().to_string();
        let position = Position {
            id: position_id.clone(),
            instrument_id,
            quantity,
            average_cost,
            entry_date: Utc::now(),
        };
        
        self.positions.push(position);
        self.updated_at = Utc::now();
        position_id
    }

    pub fn update_position(&mut self, position_id: &str, quantity: f64) -> Result<()> {
        let position = self.positions.iter_mut()
            .find(|p| p.id == position_id)
            .ok_or_else(|| ValuationError::Portfolio(format!("Position not found: {}", position_id)))?;
        
        position.quantity = quantity;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn remove_position(&mut self, position_id: &str) -> Result<()> {
        let index = self.positions.iter()
            .position(|p| p.id == position_id)
            .ok_or_else(|| ValuationError::Portfolio(format!("Position not found: {}", position_id)))?;
        
        self.positions.remove(index);
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn get_total_positions(&self) -> usize {
        self.positions.len()
    }

    pub fn get_position_by_instrument(&self, instrument_id: &str) -> Option<&Position> {
        self.positions.iter().find(|p| p.instrument_id == instrument_id)
    }
}

pub struct PortfolioValuationService {
    risk_engine: RiskEngine,
}

impl PortfolioValuationService {
    pub fn new(risk_engine: RiskEngine) -> Self {
        Self { risk_engine }
    }

    pub async fn value_portfolio(
        &self,
        portfolio: &Portfolio,
        instruments: &HashMap<String, Box<dyn Instrument + Send + Sync>>,
        valuator: &dyn Valuator,
        market_context: &MarketContext,
    ) -> Result<PortfolioValuation> {
        let mut position_valuations = Vec::new();
        let mut total_value = 0.0;

        // Value each position
        for position in &portfolio.positions {
            let instrument = instruments.get(&position.instrument_id)
                .ok_or_else(|| ValuationError::Portfolio(
                    format!("Instrument not found: {}", position.instrument_id)
                ))?;

            let valuation_result = valuator.value(instrument.as_ref(), market_context)?;
            let unit_value = valuation_result.value / instrument.notional();
            let position_total_value = unit_value * position.quantity;
            
            // Calculate P&L if we have average cost
            let (pnl, pnl_percentage) = if let Some(avg_cost) = position.average_cost {
                let total_cost = avg_cost * position.quantity;
                let pnl = position_total_value - total_cost;
                let pnl_pct = if total_cost != 0.0 { pnl / total_cost * 100.0 } else { 0.0 };
                (Some(pnl), Some(pnl_pct))
            } else {
                (None, None)
            };

            position_valuations.push(PositionValuation {
                position_id: position.id.clone(),
                instrument_id: position.instrument_id.clone(),
                quantity: position.quantity,
                unit_value,
                total_value: position_total_value,
                weight: 0.0, // Will be calculated after total value is known
                pnl,
                pnl_percentage,
                valuation_result,
            });

            total_value += position_total_value;
        }

        // Calculate weights
        for position_val in &mut position_valuations {
            position_val.weight = if total_value != 0.0 {
                position_val.total_value / total_value * 100.0
            } else {
                0.0
            };
        }

        // Calculate portfolio risk metrics
        let risk_metrics = self.calculate_portfolio_risk_metrics(
            &position_valuations,
            total_value,
            market_context,
        ).ok();

        // Calculate performance metrics
        let performance = self.calculate_portfolio_performance(&position_valuations);

        Ok(PortfolioValuation {
            portfolio_id: portfolio.id.clone(),
            total_value,
            currency: portfolio.base_currency.clone(),
            positions: position_valuations,
            risk_metrics,
            timestamp: Utc::now(),
            performance,
        })
    }

    fn calculate_portfolio_risk_metrics(
        &self,
        positions: &[PositionValuation],
        total_value: f64,
        _market_context: &MarketContext,
    ) -> Result<RiskMetrics> {
        if positions.is_empty() || total_value == 0.0 {
            return Ok(RiskMetrics {
                var_1d: None,
                var_10d: None,
                expected_shortfall: None,
                volatility: None,
            });
        }

        // Calculate portfolio volatility as weighted average (simplified)
        let mut portfolio_volatility = 0.0;
        let mut total_weight = 0.0;

        for position in positions {
            if let Some(vol) = position.valuation_result.risk_metrics.as_ref()
                .and_then(|rm| rm.volatility) {
                let weight = position.total_value / total_value;
                portfolio_volatility += weight * vol;
                total_weight += weight;
            }
        }

        if total_weight > 0.0 {
            portfolio_volatility /= total_weight;
        } else {
            portfolio_volatility = 0.20; // Default 20% volatility
        }

        // Use risk engine to calculate portfolio risk metrics
        self.risk_engine.calculate_portfolio_risk_metrics(
            total_value,
            portfolio_volatility,
            0.08, // Assume 8% expected return
        )
    }

    fn calculate_portfolio_performance(&self, positions: &[PositionValuation]) -> Option<PortfolioPerformance> {
        if positions.is_empty() {
            return None;
        }

        let mut total_cost = 0.0;
        let mut total_value = 0.0;
        let mut has_cost_data = false;

        for position in positions {
            total_value += position.total_value;
            if let Some(pnl) = position.pnl {
                total_cost += position.total_value - pnl;
                has_cost_data = true;
            }
        }

        if !has_cost_data || total_cost == 0.0 {
            return None;
        }

        let total_return = total_value - total_cost;
        let total_return_percentage = total_return / total_cost * 100.0;

        Some(PortfolioPerformance {
            total_return,
            total_return_percentage,
            daily_return: None, // Would need historical data
            daily_return_percentage: None,
            sharpe_ratio: None, // Would need risk-free rate and historical returns
            max_drawdown: None, // Would need historical data
            volatility: None, // Would need historical returns
        })
    }

    pub fn calculate_portfolio_attribution(
        &self,
        current_valuation: &PortfolioValuation,
        previous_valuation: &PortfolioValuation,
    ) -> Result<PortfolioAttribution> {
        let mut attributions = Vec::new();
        let total_return = current_valuation.total_value - previous_valuation.total_value;

        for current_pos in &current_valuation.positions {
            if let Some(previous_pos) = previous_valuation.positions.iter()
                .find(|p| p.instrument_id == current_pos.instrument_id) {
                
                let position_return = current_pos.total_value - previous_pos.total_value;
                let contribution = if previous_valuation.total_value != 0.0 {
                    position_return / previous_valuation.total_value * 100.0
                } else {
                    0.0
                };

                attributions.push(PositionAttribution {
                    instrument_id: current_pos.instrument_id.clone(),
                    contribution,
                    position_return,
                    weight_effect: 0.0, // Simplified - would need more complex calculation
                    selection_effect: contribution, // Simplified
                });
            }
        }

        Ok(PortfolioAttribution {
            total_return,
            total_return_percentage: if previous_valuation.total_value != 0.0 {
                total_return / previous_valuation.total_value * 100.0
            } else {
                0.0
            },
            position_attributions: attributions,
            timestamp: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAttribution {
    pub total_return: f64,
    pub total_return_percentage: f64,
    pub position_attributions: Vec<PositionAttribution>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionAttribution {
    pub instrument_id: String,
    pub contribution: f64, // Contribution to total portfolio return (%)
    pub position_return: f64,
    pub weight_effect: f64,
    pub selection_effect: f64,
}

impl Default for PortfolioValuationService {
    fn default() -> Self {
        Self::new(RiskEngine::default())
    }
}
