use std::collections::HashMap;
use anyhow::{Result, anyhow};
use colored::Colorize;
use anchor_client::solana_sdk::pubkey::Pubkey;

use crate::common::logger::Logger;
use crate::dex::pump_fun::Pump;

/// Represents the bonding curve parameters for a token
#[derive(Debug, Clone)]
pub struct BondingCurve {
    /// Current liquidity in SOL
    pub liquidity: f64,
    /// How quickly price rises with buys (steepness)
    pub curve_steepness: f64,
    /// Price at 0 tokens bought
    pub initial_price: f64,
    /// Current token price
    pub current_price: f64,
    /// Last calculated liquidity depth
    pub liquidity_depth: f64,
    /// History of liquidity changes
    pub liquidity_history: Vec<(u64, f64)>, // timestamp, liquidity
}

impl BondingCurve {
    /// Create a new bonding curve analysis
    pub fn new(initial_price: f64, current_price: f64, liquidity: f64) -> Self {
        // Estimate steepness based on initial and current price
        let curve_steepness = if initial_price > 0.0 && current_price > initial_price {
            (current_price / initial_price - 1.0) / liquidity
        } else {
            0.05 // Default fallback value
        };
        
        Self {
            liquidity,
            curve_steepness,
            initial_price,
            current_price,
            liquidity_depth: liquidity,
            liquidity_history: vec![(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                liquidity
            )],
        }
    }
    
    /// Calculate price impact for a given buy amount in SOL
    pub fn calculate_price_impact(&self, buy_amount_sol: f64) -> f64 {
        if self.liquidity == 0.0 || buy_amount_sol == 0.0 {
            return 0.0;
        }
        
        // Simple bonding curve model: price impact increases with buy size relative to liquidity
        let relative_size = buy_amount_sol / self.liquidity;
        let impact = relative_size * self.curve_steepness * 100.0;
        
        // Cap at 100% to avoid unrealistic values
        impact.min(100.0)
    }
    
    /// Calculate optimal trade size to limit price impact
    pub fn calculate_optimal_trade_size(&self, max_price_impact_pct: f64) -> f64 {
        if self.curve_steepness == 0.0 || max_price_impact_pct <= 0.0 {
            return 0.0;
        }
        
        // Solve for buy_amount: max_price_impact = (buy_amount/liquidity) * curve_steepness * 100
        // buy_amount = (max_price_impact * liquidity) / (curve_steepness * 100)
        let optimal_size = (max_price_impact_pct * self.liquidity) / (self.curve_steepness * 100.0);
        
        // Return a reasonable value capped at 90% of liquidity
        optimal_size.min(self.liquidity * 0.9)
    }
    
    /// Calculate estimated price after a buy of a specific size
    pub fn estimate_price_after_buy(&self, buy_amount_sol: f64) -> f64 {
        let price_impact_pct = self.calculate_price_impact(buy_amount_sol);
        self.current_price * (1.0 + price_impact_pct / 100.0)
    }
    
    /// Update the bonding curve with new liquidity data
    pub fn update_liquidity(&mut self, new_liquidity: f64) {
        self.liquidity = new_liquidity;
        
        // Add to history
        self.liquidity_history.push((
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            new_liquidity
        ));
        
        // Keep history manageable
        if self.liquidity_history.len() > 100 {
            self.liquidity_history.remove(0);
        }
        
        // Update liquidity depth as a simple average of recent measurements
        if !self.liquidity_history.is_empty() {
            self.liquidity_depth = self.liquidity_history
                .iter()
                .map(|(_, liquidity)| *liquidity)
                .sum::<f64>() / self.liquidity_history.len() as f64;
        }
    }
    
    /// Calculate liquidity growth rate over the last N data points
    pub fn calculate_liquidity_growth_rate(&self, lookback_points: usize) -> f64 {
        let history_len = self.liquidity_history.len();
        if history_len < 2 || lookback_points < 2 {
            return 0.0;
        }
        
        let points_to_use = lookback_points.min(history_len);
        let start_idx = history_len - points_to_use;
        
        let oldest_liquidity = self.liquidity_history[start_idx].1;
        let newest_liquidity = self.liquidity_history[history_len - 1].1;
        
        if oldest_liquidity == 0.0 {
            return 0.0;
        }
        
        ((newest_liquidity / oldest_liquidity) - 1.0) * 100.0
    }
}

/// Manager for tracking and analyzing token bonding curves
pub struct BondingCurveManager {
    /// Bonding curves for each token mint
    curves: HashMap<String, BondingCurve>,
    /// Logger for events
    logger: Logger,
}

impl BondingCurveManager {
    /// Create a new bonding curve manager
    pub fn new(logger: Logger) -> Self {
        Self {
            curves: HashMap::new(),
            logger,
        }
    }
    
    /// Get or create bonding curve for a token
    pub fn get_or_create_curve(&mut self, token_mint: &str, initial_price: f64, current_price: f64, liquidity: f64) -> &mut BondingCurve {
        if !self.curves.contains_key(token_mint) {
            let curve = BondingCurve::new(initial_price, current_price, liquidity);
            self.curves.insert(token_mint.to_string(), curve);
        }
        
        self.curves.get_mut(token_mint).unwrap()
    }
    
    /// Update a token's bonding curve with new data
    pub fn update_curve(&mut self, token_mint: &str, current_price: f64, current_liquidity: f64) -> Result<()> {
        if let Some(curve) = self.curves.get_mut(token_mint) {
            // Update price
            curve.current_price = current_price;
            
            // Update liquidity
            curve.update_liquidity(current_liquidity);
            
            // Recalculate curve steepness if possible
            if curve.initial_price > 0.0 && current_price > curve.initial_price && current_liquidity > 0.0 {
                curve.curve_steepness = (current_price / curve.initial_price - 1.0) / current_liquidity;
            }
            
            Ok(())
        } else {
            Err(anyhow!("No curve exists for token {}", token_mint))
        }
    }
    
    /// Get price impact analysis for different trade sizes
    pub fn get_price_impact_analysis(&self, token_mint: &str) -> Result<BondingCurveAnalysis> {
        let curve = self.curves.get(token_mint)
            .ok_or_else(|| anyhow!("No curve exists for token {}", token_mint))?;
            
        // Calculate optimal trade size for 5% price impact
        let optimal_trade_size = curve.calculate_optimal_trade_size(5.0);
        
        // Calculate impact for standard sizes
        let impact_1sol = curve.calculate_price_impact(1.0);
        let impact_5sol = curve.calculate_price_impact(5.0);
        
        Ok(BondingCurveAnalysis {
            token_mint: token_mint.to_string(),
            liquidity: curve.liquidity,
            liquidity_depth: curve.liquidity_depth,
            curve_steepness: curve.curve_steepness,
            optimal_trade_size,
            price_impact_1sol: impact_1sol,
            price_impact_5sol: impact_5sol,
            liquidity_growth_rate: curve.calculate_liquidity_growth_rate(5),
        })
    }
    
    /// Analyze transaction for its impact on curve and price
    pub async fn analyze_transaction(
        &mut self,
        token_mint: &str,
        transaction_amount_sol: f64,
        is_buy: bool,
        swapx: &Pump,
    ) -> Result<TransactionImpactAnalysis> {
        // Default values
        let mut current_liquidity = 0.0;
        let mut current_price = 0.0;
        
        // Try to get actual data from DEX
        if let Ok(pool_info) = swapx.get_token_pool_info(&token_mint.parse::<Pubkey>().unwrap()).await {
            current_liquidity = pool_info.liquidity;
            current_price = pool_info.price;
        }
        
        // Get or create curve
        let curve = self.get_or_create_curve(token_mint, 0.0, current_price, current_liquidity);
        
        // Update curve with new data
        curve.current_price = current_price;
        curve.update_liquidity(current_liquidity);
        
        // Calculate expected price impact
        let price_impact = if is_buy {
            curve.calculate_price_impact(transaction_amount_sol)
        } else {
            // Sell impact is typically larger and in the opposite direction
            -curve.calculate_price_impact(transaction_amount_sol) * 1.2
        };
        
        // Calculate projected new price
        let projected_price = current_price * (1.0 + price_impact / 100.0);
        
        // Calculate liquidity change
        let liquidity_change = if is_buy {
            transaction_amount_sol
        } else {
            -transaction_amount_sol
        };
        
        let projected_liquidity = (current_liquidity + liquidity_change).max(0.0);
        
        // Log the analysis
        self.logger.log(format!(
            "Transaction impact analysis for {}: {} SOL {} - Price impact: {}%, New price: {} SOL, New liquidity: {} SOL",
            token_mint,
            transaction_amount_sol,
            if is_buy { "BUY".green() } else { "SELL".red() },
            price_impact,
            projected_price,
            projected_liquidity
        ).cyan().to_string());
        
        Ok(TransactionImpactAnalysis {
            token_mint: token_mint.to_string(),
            transaction_amount_sol,
            is_buy,
            current_price,
            projected_price,
            price_impact_pct: price_impact,
            current_liquidity,
            projected_liquidity,
        })
    }
}

/// Analysis of a bonding curve
#[derive(Debug, Clone)]
pub struct BondingCurveAnalysis {
    /// Token mint address
    pub token_mint: String,
    /// Current liquidity in SOL
    pub liquidity: f64,
    /// Liquidity depth (average of recent measurements)
    pub liquidity_depth: f64,
    /// Steepness of the bonding curve
    pub curve_steepness: f64,
    /// Optimal trade size for minimal impact (SOL)
    pub optimal_trade_size: f64,
    /// Price impact for 1 SOL buy (%)
    pub price_impact_1sol: f64,
    /// Price impact for 5 SOL buy (%)
    pub price_impact_5sol: f64,
    /// Liquidity growth rate (%)
    pub liquidity_growth_rate: f64,
}

/// Analysis of a transaction's impact on price and liquidity
#[derive(Debug, Clone)]
pub struct TransactionImpactAnalysis {
    /// Token mint address
    pub token_mint: String,
    /// Transaction amount in SOL
    pub transaction_amount_sol: f64,
    /// Whether the transaction is a buy
    pub is_buy: bool,
    /// Current token price
    pub current_price: f64,
    /// Projected price after transaction
    pub projected_price: f64,
    /// Price impact as a percentage
    pub price_impact_pct: f64,
    /// Current liquidity in SOL
    pub current_liquidity: f64,
    /// Projected liquidity after transaction
    pub projected_liquidity: f64,
} 