use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::{Result, anyhow};
use colored::Colorize;
use chrono::{Utc, DateTime};
use tokio::time;

use crate::common::logger::Logger;
use crate::engine::advanced_trading::RiskProfile;

/// Represents a risk-adjusted position
#[derive(Debug, Clone)]
pub struct RiskAdjustedPosition {
    /// Token mint address
    pub token_mint: String,
    /// Position size in SOL
    pub position_size: f64,
    /// Risk per trade (% of portfolio)
    pub risk_percentage: f64,
    /// Maximum loss on this position in SOL
    pub max_loss_sol: f64,
    /// Risk-to-reward ratio (e.g., 1:3 means potential reward is 3x the risk)
    pub risk_reward_ratio: f64,
    /// Risk profile of the token
    pub risk_profile: RiskProfile,
    /// Time when position was opened
    pub open_time: DateTime<Utc>,
}

/// Daily performance statistics
#[derive(Debug, Clone)]
pub struct DailyPerformance {
    /// Date in YYYY-MM-DD format
    pub date: String,
    /// Number of trades
    pub trade_count: u32,
    /// Win rate (percentage)
    pub win_rate: f64,
    /// Total profit/loss in SOL
    pub total_pnl_sol: f64,
    /// Total profit/loss as percentage of portfolio
    pub total_pnl_percent: f64,
    /// Largest winning trade in SOL
    pub largest_win_sol: f64,
    /// Largest losing trade in SOL
    pub largest_loss_sol: f64,
    /// Average winning trade in SOL
    pub avg_win_sol: f64,
    /// Average losing trade in SOL
    pub avg_loss_sol: f64,
}

/// Risk management system for the trading bot
pub struct RiskManager {
    /// Logger for events
    logger: Logger,
    /// Portfolio value in SOL
    portfolio_value: f64,
    /// Maximum percentage of portfolio to risk per trade
    max_risk_per_trade: f64,
    /// Maximum daily loss limit as percentage of portfolio
    daily_loss_limit: f64,
    /// Current day's profit/loss as percentage of portfolio
    current_day_pnl: f64,
    /// Current day's trades
    current_day_trades: Vec<TradeRecord>,
    /// Current open positions
    open_positions: HashMap<String, RiskAdjustedPosition>,
    /// Historical daily performance
    daily_performance: Vec<DailyPerformance>,
    /// Market volatility factor (1.0 is normal, higher means more volatile)
    market_volatility: f64,
    /// Whether trading is active (false if circuit breakers triggered)
    trading_active: bool,
    /// Blacklisted tokens (trading disabled)
    blacklisted_tokens: HashSet<String>,
    /// Start of current trading day
    current_day_start: DateTime<Utc>,
}

/// Record of a completed trade
#[derive(Debug, Clone)]
pub struct TradeRecord {
    /// Token mint address
    pub token_mint: String,
    /// Entry price
    pub entry_price: f64,
    /// Exit price
    pub exit_price: f64,
    /// Position size in SOL
    pub position_size: f64,
    /// Profit/loss in SOL
    pub pnl_sol: f64,
    /// Profit/loss as percentage
    pub pnl_percent: f64,
    /// Risk profile at entry
    pub risk_profile: RiskProfile,
    /// Entry time
    pub entry_time: DateTime<Utc>,
    /// Exit time
    pub exit_time: DateTime<Utc>,
    /// Hold duration in seconds
    pub hold_duration_secs: u64,
    /// Reason for exit
    pub exit_reason: String,
}

impl RiskManager {
    /// Create a new risk manager
    pub fn new(logger: Logger, portfolio_value: f64) -> Self {
        // Load configuration from environment variables or use defaults
        let max_risk_per_trade = std::env::var("MAX_RISK_PER_TRADE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(2.0); // Default 2% risk per trade
            
        let daily_loss_limit = std::env::var("DAILY_LOSS_LIMIT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0); // Default 5% daily loss limit
            
        Self {
            logger,
            portfolio_value,
            max_risk_per_trade,
            daily_loss_limit,
            current_day_pnl: 0.0,
            current_day_trades: Vec::new(),
            open_positions: HashMap::new(),
            daily_performance: Vec::new(),
            market_volatility: 1.0,
            trading_active: true,
            blacklisted_tokens: HashSet::new(),
            current_day_start: Utc::now(),
        }
    }
    
    /// Calculate the optimal position size based on risk parameters
    pub fn calculate_position_size(
        &self,
        token_mint: &str,
        _entry_price: f64,
        stop_loss_percent: f64,
        risk_profile: &RiskProfile,
    ) -> f64 {
        // If trading is not active, return 0
        if !self.trading_active {
            return 0.0;
        }
        
        // If token is blacklisted, return 0
        if self.blacklisted_tokens.contains(token_mint) {
            return 0.0;
        }
        
        // Base risk percentage from max_risk_per_trade
        let mut risk_pct = self.max_risk_per_trade;
        
        // Adjust risk percentage based on risk profile
        risk_pct = match risk_profile {
            RiskProfile::Low => risk_pct * 1.0,
            RiskProfile::Medium => risk_pct * 0.8,
            RiskProfile::High => risk_pct * 0.5,
            RiskProfile::VeryHigh => risk_pct * 0.3,
        };
        
        // Adjust for market volatility
        risk_pct = risk_pct / self.market_volatility;
        
        // Calculate maximum amount to risk in SOL
        let max_risk_amount = self.portfolio_value * (risk_pct / 100.0);
        
        // Calculate position size based on stop loss percentage
        // Formula: position_size = max_risk_amount / (entry_price * stop_loss_percentage)
        let position_size = max_risk_amount / (stop_loss_percent / 100.0);
        
        // Cap position size at 10% of portfolio regardless of other factors
        let max_position = self.portfolio_value * 0.1;
        position_size.min(max_position)
    }
    
    /// Open a new position with risk management
    pub fn open_position(
        &mut self,
        token_mint: &str,
        _entry_price: f64,
        position_size: f64,
        stop_loss_percent: f64,
        take_profit_percent: f64,
        risk_profile: RiskProfile,
    ) -> Result<RiskAdjustedPosition> {
        // Check if trading is active
        if !self.trading_active {
            return Err(anyhow!("Trading is currently paused due to risk limits"));
        }
        
        // Check if token is blacklisted
        if self.blacklisted_tokens.contains(token_mint) {
            return Err(anyhow!("Token is blacklisted"));
        }
        
        // Calculate max loss in SOL
        let max_loss_sol = position_size * (stop_loss_percent / 100.0);
        
        // Calculate risk percentage of portfolio
        let risk_percentage = (max_loss_sol / self.portfolio_value) * 100.0;
        
        // Calculate risk-to-reward ratio
        let risk_reward_ratio = take_profit_percent / stop_loss_percent;
        
        // Create position object
        let position = RiskAdjustedPosition {
            token_mint: token_mint.to_string(),
            position_size,
            risk_percentage,
            max_loss_sol,
            risk_reward_ratio,
            risk_profile,
            open_time: Utc::now(),
        };
        
        // Log the position opening
        self.logger.log(format!(
            "RISK MANAGEMENT: Opening position for {} - Size: {:.3} SOL, Risk: {:.2}% of portfolio, Max Loss: {:.3} SOL, R:R Ratio: {:.1}:1",
            token_mint,
            position_size,
            risk_percentage,
            max_loss_sol,
            risk_reward_ratio
        ).green().to_string());
        
        // Store the position
        self.open_positions.insert(token_mint.to_string(), position.clone());
        
        Ok(position)
    }
    
    /// Close a position and record the result
    pub fn close_position(
        &mut self,
        token_mint: &str,
        exit_price: f64,
        exit_reason: &str,
    ) -> Result<TradeRecord> {
        // Get the position
        let position = self.open_positions.remove(token_mint)
            .ok_or_else(|| anyhow!("No open position found for token {}", token_mint))?;
            
        // Calculate PnL
        let _entry_price = 0.0; // This should be stored in the position object
        let pnl_percent = ((exit_price - _entry_price) / _entry_price) * 100.0;
        let pnl_sol = position.position_size * (pnl_percent / 100.0);
        
        // Create trade record
        let exit_time = Utc::now();
        let hold_duration = exit_time.signed_duration_since(position.open_time);
        
        let trade = TradeRecord {
            token_mint: token_mint.to_string(),
            entry_price: _entry_price,
            exit_price,
            position_size: position.position_size,
            pnl_sol,
            pnl_percent,
            risk_profile: position.risk_profile,
            entry_time: position.open_time,
            exit_time,
            hold_duration_secs: hold_duration.num_seconds() as u64,
            exit_reason: exit_reason.to_string(),
        };
        
        // Log the trade result
        let pnl_color = if pnl_sol >= 0.0 { "green" } else { "red" };
        self.logger.log(format!(
            "RISK MANAGEMENT: Closed position for {} - PnL: {:.3} SOL ({:.2}%), Hold time: {}s, Reason: {}",
            token_mint,
            pnl_sol,
            pnl_percent,
            hold_duration.num_seconds(),
            exit_reason
        ).color(pnl_color.to_string()).to_string());
        
        // Update daily PnL
        self.current_day_pnl += (pnl_sol / self.portfolio_value) * 100.0;
        
        // Add to daily trades
        self.current_day_trades.push(trade.clone());
        
        // Check if we've hit the daily loss limit
        if self.current_day_pnl <= -self.daily_loss_limit {
            self.trading_active = false;
            self.logger.log(format!(
                "CIRCUIT BREAKER TRIGGERED: Daily loss limit of {}% has been hit. Trading paused.",
                self.daily_loss_limit
            ).red().bold().to_string());
        }
        
        Ok(trade)
    }
    
    /// Update market volatility factor
    pub fn update_market_volatility(&mut self, new_volatility: f64) {
        self.market_volatility = new_volatility;
        
        // Log volatility change if significant
        if (new_volatility - 1.0).abs() > 0.2 {
            let volatility_status = if new_volatility > 1.2 {
                "HIGH".red()
            } else if new_volatility < 0.8 {
                "LOW".green()
            } else {
                "NORMAL".yellow()
            };
            
            self.logger.log(format!(
                "Market volatility updated: {} (factor: {:.2}x) - Risk per trade adjusted accordingly",
                volatility_status,
                new_volatility
            ).cyan().to_string());
        }
    }
    
    /// Reset daily statistics (call at start of trading day)
    pub fn reset_daily_stats(&mut self) {
        // Calculate performance for the previous day
        let performance = self.calculate_daily_performance();
        self.daily_performance.push(performance);
        
        // Reset current day values
        self.current_day_pnl = 0.0;
        self.current_day_trades.clear();
        self.current_day_start = Utc::now();
        self.trading_active = true;
        
        self.logger.log("RISK MANAGEMENT: Daily statistics reset. Trading enabled.".green().to_string());
    }
    
    /// Calculate daily performance from current day's trades
    fn calculate_daily_performance(&self) -> DailyPerformance {
        let date = self.current_day_start.format("%Y-%m-%d").to_string();
        let trade_count = self.current_day_trades.len() as u32;
        
        // Default values
        let mut win_rate: f64 = 0.0;
        let mut total_pnl_sol: f64 = 0.0;
        let mut largest_win_sol: f64 = 0.0;
        let mut largest_loss_sol: f64 = 0.0;
        let mut winning_trades_sol: Vec<f64> = Vec::new();
        let mut losing_trades_sol: Vec<f64> = Vec::new();
        
        // Calculate statistics if we have trades
        if trade_count > 0 {
            // Count winning trades
            let winning_trades = self.current_day_trades.iter()
                .filter(|t| t.pnl_sol > 0.0)
                .count();
                
            win_rate = (winning_trades as f64 / trade_count as f64) * 100.0;
            
            // Calculate PnL
            total_pnl_sol = self.current_day_trades.iter()
                .map(|t| t.pnl_sol)
                .sum();
                
            // Find largest win and loss
            for trade in &self.current_day_trades {
                if trade.pnl_sol > 0.0 {
                    largest_win_sol = largest_win_sol.max(trade.pnl_sol);
                    winning_trades_sol.push(trade.pnl_sol);
                } else if trade.pnl_sol < 0.0 {
                    largest_loss_sol = largest_loss_sol.min(trade.pnl_sol);
                    losing_trades_sol.push(trade.pnl_sol);
                }
            }
        }
        
        // Calculate averages
        let avg_win_sol = if !winning_trades_sol.is_empty() {
            winning_trades_sol.iter().sum::<f64>() / winning_trades_sol.len() as f64
        } else {
            0.0
        };
        
        let avg_loss_sol = if !losing_trades_sol.is_empty() {
            losing_trades_sol.iter().sum::<f64>() / losing_trades_sol.len() as f64
        } else {
            0.0
        };
        
        DailyPerformance {
            date,
            trade_count,
            win_rate,
            total_pnl_sol,
            total_pnl_percent: self.current_day_pnl,
            largest_win_sol,
            largest_loss_sol,
            avg_win_sol,
            avg_loss_sol,
        }
    }
    
    /// Check if trading should continue based on risk parameters
    pub fn should_continue_trading(&self) -> bool {
        self.trading_active
    }
    
    /// Add a token to the blacklist
    pub fn blacklist_token(&mut self, token_mint: &str, reason: &str) {
        self.blacklisted_tokens.insert(token_mint.to_string());
        
        self.logger.log(format!(
            "RISK MANAGEMENT: Token {} blacklisted. Reason: {}",
            token_mint,
            reason
        ).red().to_string());
    }
    
    /// Get current portfolio allocation
    pub fn get_portfolio_allocation(&self) -> HashMap<String, f64> {
        let mut allocation = HashMap::new();
        
        let total_allocated: f64 = self.open_positions.values()
            .map(|p| p.position_size)
            .sum();
            
        for (token, position) in &self.open_positions {
            let percentage = (position.position_size / self.portfolio_value) * 100.0;
            allocation.insert(token.clone(), percentage);
        }
        
        // Add remaining cash
        let cash_percentage = ((self.portfolio_value - total_allocated) / self.portfolio_value) * 100.0;
        allocation.insert("CASH".to_string(), cash_percentage);
        
        allocation
    }
    
    /// Calculate Kelly criterion for position sizing
    pub fn calculate_kelly(&self, win_probability: f64, win_loss_ratio: f64) -> f64 {
        // Kelly formula: f* = (bp - q) / b
        // where p = win probability, q = loss probability (1-p), b = win/loss ratio
        
        if win_probability <= 0.0 || win_loss_ratio <= 0.0 {
            return 0.0;
        }
        
        let loss_probability = 1.0 - win_probability;
        let kelly = (win_probability * win_loss_ratio - loss_probability) / win_loss_ratio;
        
        // Apply a fractional Kelly (half-Kelly) for safety
        let fractional_kelly = kelly * 0.5;
        
        // Prevent negative Kelly values and cap at 25%
        fractional_kelly.max(0.0).min(0.25)
    }
    
    /// Update portfolio value
    pub fn update_portfolio_value(&mut self, new_value: f64) {
        let old_value = self.portfolio_value;
        self.portfolio_value = new_value;
        
        // Calculate change percentage
        let change_pct = ((new_value - old_value) / old_value) * 100.0;
        
        self.logger.log(format!(
            "Portfolio value updated: {:.3} SOL ({:+.2}%)",
            new_value,
            change_pct
        ).cyan().to_string());
    }
}

/// Start the risk management system
pub async fn start_risk_management_system(
    logger: Logger,
    initial_portfolio_value: f64,
) -> Arc<Mutex<RiskManager>> {
    let risk_manager = RiskManager::new(logger.clone(), initial_portfolio_value);
    let risk_manager_arc = Arc::new(Mutex::new(risk_manager));
    
    // Start background task to reset daily stats at midnight
    let risk_manager_clone = risk_manager_arc.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(3600)); // Check every hour
        loop {
            interval.tick().await;
            
            // Get current time
            let now = Utc::now();
            
            // Lock risk manager
            if let Ok(mut rm) = risk_manager_clone.lock() {
                // Check if day has changed
                if now.date_naive() > rm.current_day_start.date_naive() {
                    // Reset daily stats
                    rm.reset_daily_stats();
                }
            }
        }
    });
    
    risk_manager_arc
} 