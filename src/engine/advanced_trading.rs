use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Result;
use colored::Colorize;
use tokio::time;

use crate::common::logger::Logger;
use crate::dex::pump_fun::Pump;
use crate::common::config::LiquidityPool;
use crate::engine::token_tracker::{TokenTracker, ExtendedTokenInfo};
use crate::engine::token_selling::SellManager;
use crate::services::telegram::TelegramService;

/// Advanced risk profile for tokens
#[derive(Debug, Clone)]
pub enum RiskProfile {
    Low,
    Medium,
    High,
    VeryHigh,
}

/// Technical indicators for entry and exit decisions
#[derive(Debug, Clone)]
pub struct TechnicalIndicators {
    /// Momentum indicator: positive values indicate uptrend
    pub momentum: f64,
    /// Volatility measure (higher = more volatile)
    pub volatility: f64,
    /// Buy/sell ratio (above 1.0 means more buys than sells)
    pub buy_sell_ratio: f64,
    /// Liquidity growth rate (percentage change in liquidity)
    pub liquidity_growth_rate: f64,
    /// Market sentiment score (0-100, higher is more bullish)
    pub market_sentiment: f64,
}

/// Bonding curve analysis for price impact calculation
#[derive(Debug, Clone)]
pub struct BondingCurveAnalysis {
    /// Steepness of the bonding curve (higher = more price impact)
    pub curve_steepness: f64,
    /// Liquidity depth in SOL
    pub liquidity_depth: f64,
    /// Max recommended trade size in SOL before significant slippage
    pub optimal_trade_size: f64,
    /// Price impact for 1 SOL buy
    pub price_impact_1sol: f64,
    /// Price impact for 5 SOL buy
    pub price_impact_5sol: f64,
}

/// Advanced token entry criteria with multiple signals
#[derive(Debug, Clone)]
pub struct EntryCriteria {
    /// Primary risk profile for the token
    pub risk_profile: RiskProfile,
    /// Technical indicators for entry signals
    pub indicators: TechnicalIndicators,
    /// Whether the token meets the threshold for a buy signal
    pub has_buy_signal: bool,
    /// Bonding curve analysis for price impact
    pub curve_analysis: Option<BondingCurveAnalysis>,
    /// Launch recency (seconds since token creation)
    pub token_age_seconds: u64,
    /// Confidence score (0-100) for entry success probability
    pub entry_confidence: u64,
}

/// Dynamic exit strategy with multiple take-profit levels
#[derive(Debug, Clone)]
pub struct DynamicExitStrategy {
    /// Initial take-profit percentage
    pub initial_take_profit_pct: f64,
    /// Take-profit levels (percentage, amount to sell)
    pub take_profit_levels: Vec<(f64, f64)>,
    /// Initial stop-loss percentage
    pub initial_stop_loss_pct: f64,
    /// Trailing stop distance (%)
    pub trailing_stop_distance: f64,
    /// Time-based exit thresholds (seconds, percentage to sell)
    pub time_based_exits: Vec<(u64, f64)>,
    /// Risk-adjusted stop-loss (tighter for higher risk tokens)
    pub risk_adjusted_stop_loss: f64,
    /// Current drawdown from peak price
    pub current_drawdown: f64,
    /// Whether to increase position if positive indicators appear
    pub enable_position_scaling: bool,
}

/// Advanced trading manager implementing sophisticated strategies
#[allow(dead_code)]
pub struct AdvancedTradingManager {
    /// Logger for trading events
    logger: Logger,
    /// Entry criteria for tokens being monitored
    entry_criteria: HashMap<String, EntryCriteria>,
    /// Exit strategies for tokens in portfolio
    exit_strategies: HashMap<String, DynamicExitStrategy>,
    /// Historical performance metrics for self-tuning
    performance_metrics: HashMap<String, Vec<f64>>,
    /// Kelly criterion position sizes
    kelly_position_sizes: HashMap<String, f64>,
    /// Maximum daily loss limit (%)
    daily_loss_limit: f64,
    /// Current day's running P&L (%)
    current_day_pnl: f64,
    /// Current portfolio value in SOL
    portfolio_value: f64,
    /// Current market sentiment (0-100)
    market_sentiment: f64,
    /// Whether trading is active or paused due to circuit breakers
    trading_active: bool,
}

impl AdvancedTradingManager {
    /// Create a new advanced trading manager
    pub fn new(logger: Logger) -> Self {
        // Load configuration from environment variables or use defaults
        let daily_loss_limit = std::env::var("DAILY_LOSS_LIMIT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0); // Default 5% daily loss limit
            
        Self {
            logger,
            entry_criteria: HashMap::new(),
            exit_strategies: HashMap::new(),
            performance_metrics: HashMap::new(),
            kelly_position_sizes: HashMap::new(),
            daily_loss_limit,
            current_day_pnl: 0.0,
            portfolio_value: 0.0,
            market_sentiment: 50.0, // Neutral by default
            trading_active: true,
        }
    }
    
    /// Calculate risk profile based on token metrics
    pub fn calculate_risk_profile(
        &self,
        market_cap: f64,
        volume: f64,
        buy_sell_ratio: f64,
        token_age_seconds: u64,
        _launcher_sol: f64,
    ) -> RiskProfile {
        // Higher risk for very new tokens
        if token_age_seconds < 300 { // Less than 5 minutes old
            return RiskProfile::VeryHigh;
        }
        
        // Higher risk for low market cap tokens
        if market_cap < 10.0 {
            return RiskProfile::High;
        }
        
        // Lower risk for tokens with good buy/sell ratio
        if buy_sell_ratio > 2.0 && volume > 50.0 && market_cap > 50.0 {
            return RiskProfile::Low;
        }
        
        // Medium risk for average metrics
        if buy_sell_ratio > 1.5 && volume > 20.0 && market_cap > 20.0 {
            return RiskProfile::Medium;
        }
        
        // Default to high risk
        RiskProfile::High
    }
    
    /// Calculate buy/sell ratio from transaction counts
    pub fn calculate_buy_sell_ratio(&self, buy_count: u32, sell_count: u32) -> f64 {
        if sell_count == 0 {
            return buy_count as f64; // Avoid division by zero
        }
        
        buy_count as f64 / sell_count as f64
    }
    
    /// Calculate entry criteria for a token
    pub async fn calculate_entry_criteria(
        &self,
        token_info: &ExtendedTokenInfo,
        _token_tracker: &TokenTracker,
    ) -> EntryCriteria {
        // Calculate buy/sell ratio
        let buy_sell_ratio = if token_info.sell_tx_num == 0 {
            token_info.buy_tx_num as f64
        } else {
            token_info.buy_tx_num as f64 / token_info.sell_tx_num as f64
        };
        
        // Calculate liquidity growth rate (placeholder - would need historical data)
        let liquidity_growth_rate = 0.0; // Would calculate from historical data
        
        // Calculate token age in seconds
        let token_age_seconds = token_info.token_mint_timestamp.elapsed().as_secs();
        
        // Calculate risk profile
        let risk_profile = self.calculate_risk_profile(
            token_info.market_cap.unwrap_or(0.0),
            token_info.volume.unwrap_or(0.0),
            buy_sell_ratio,
            token_age_seconds,
            token_info.launcher_sol_balance.unwrap_or(0.0),
        );
        
        // Calculate technical indicators
        let indicators = TechnicalIndicators {
            momentum: buy_sell_ratio - 1.0, // Positive means more buys than sells
            volatility: 0.0, // Would calculate from price history
            buy_sell_ratio,
            liquidity_growth_rate,
            market_sentiment: self.market_sentiment,
        };
        
        // Determine if we have a buy signal
        let has_buy_signal = buy_sell_ratio > 1.5 
            && token_age_seconds < 3600 // Less than 1 hour old
            && token_info.market_cap.unwrap_or(0.0) > 10.0 // Minimum market cap
            && (token_info.buy_tx_num > 5); // At least 5 buy transactions
            
        // Calculate entry confidence score (0-100)
        let entry_confidence = {
            let mut score = 50; // Start at neutral
            
            // Adjust based on buy/sell ratio
            if buy_sell_ratio > 2.0 { score += 15; }
            else if buy_sell_ratio > 1.5 { score += 10; }
            else if buy_sell_ratio < 1.0 { score -= 15; }
            
            // Adjust based on token age
            if token_age_seconds < 600 { score += 10; } // Fresh but not too new
            else if token_age_seconds > 3600 { score -= 10; } // Older than 1 hour
            
            // Adjust based on market cap
            if let Some(mc) = token_info.market_cap {
                if mc > 50.0 { score += 10; }
                else if mc < 10.0 { score -= 5; }
            }
            
            // Adjust based on market sentiment
            if self.market_sentiment > 70.0 { score += 10; }
            else if self.market_sentiment < 30.0 { score -= 10; }
            
            // Ensure score is within 0-100 range
            score.clamp(0, 100) as u64
        };
        
        EntryCriteria {
            risk_profile,
            indicators,
            has_buy_signal,
            curve_analysis: None, // Would require data from bonding curve
            token_age_seconds,
            entry_confidence,
        }
    }
    
    /// Create a dynamic exit strategy based on token risk profile
    pub fn create_exit_strategy(&self, _token_mint: &str, risk_profile: &RiskProfile, _buy_price: f64) -> DynamicExitStrategy {
        // Base take profit and stop loss percentages
        let (base_tp, base_sl, trailing_stop) = match risk_profile {
            RiskProfile::Low => (25.0, 10.0, 15.0),
            RiskProfile::Medium => (40.0, 15.0, 20.0),
            RiskProfile::High => (60.0, 20.0, 25.0),
            RiskProfile::VeryHigh => (100.0, 25.0, 35.0),
        };
        
        // Create take profit levels with percentages of position to sell
        let take_profit_levels = match risk_profile {
            RiskProfile::Low => vec![
                (base_tp, 0.3), // 30% of position at base TP
                (base_tp * 1.5, 0.3), // 30% at 1.5x base TP
                (base_tp * 2.0, 0.2), // 20% at 2x base TP
                (base_tp * 3.0, 0.2), // 20% at 3x base TP
            ],
            RiskProfile::Medium => vec![
                (base_tp * 0.5, 0.2), // 20% at half base TP
                (base_tp, 0.3), // 30% at base TP
                (base_tp * 2.0, 0.3), // 30% at 2x base TP
                (base_tp * 3.0, 0.2), // 20% at 3x base TP
            ],
            RiskProfile::High => vec![
                (base_tp * 0.3, 0.1), // 10% at 30% of base TP
                (base_tp * 0.7, 0.2), // 20% at 70% of base TP
                (base_tp, 0.3), // 30% at base TP
                (base_tp * 2.0, 0.2), // 20% at 2x base TP
                (base_tp * 4.0, 0.2), // 20% at 4x base TP
            ],
            RiskProfile::VeryHigh => vec![
                (base_tp * 0.2, 0.1), // 10% at 20% of base TP
                (base_tp * 0.5, 0.2), // 20% at 50% of base TP
                (base_tp * 1.0, 0.2), // 20% at base TP
                (base_tp * 2.0, 0.2), // 20% at 2x base TP
                (base_tp * 5.0, 0.2), // 20% at 5x base TP
                (base_tp * 10.0, 0.1), // 10% at 10x base TP
            ],
        };
        
        // Time-based exit thresholds (seconds, percentage to sell)
        let time_based_exits = match risk_profile {
            RiskProfile::Low => vec![
                (3600, 0.2), // 20% after 1 hour
                (7200, 0.3), // 30% after 2 hours
                (14400, 0.5), // 50% after 4 hours
            ],
            RiskProfile::Medium => vec![
                (1800, 0.2), // 20% after 30 minutes
                (3600, 0.3), // 30% after 1 hour
                (7200, 0.5), // 50% after 2 hours
            ],
            RiskProfile::High => vec![
                (900, 0.2), // 20% after 15 minutes
                (1800, 0.3), // 30% after 30 minutes
                (3600, 0.5), // 50% after 1 hour
            ],
            RiskProfile::VeryHigh => vec![
                (300, 0.2), // 20% after 5 minutes
                (900, 0.3), // 30% after 15 minutes
                (1800, 0.5), // 50% after 30 minutes
            ],
        };
        
        DynamicExitStrategy {
            initial_take_profit_pct: base_tp,
            take_profit_levels,
            initial_stop_loss_pct: base_sl,
            trailing_stop_distance: trailing_stop,
            time_based_exits,
            risk_adjusted_stop_loss: base_sl,
            current_drawdown: 0.0,
            enable_position_scaling: matches!(risk_profile, RiskProfile::Low | RiskProfile::Medium),
        }
    }
    
    /// Calculate position size based on Kelly criterion
    pub fn calculate_kelly_position(&self, _token_mint: &str, confidence: u64, max_position: f64) -> f64 {
        // Convert confidence (0-100) to win probability (0.0-1.0)
        let win_probability = confidence as f64 / 100.0;
        
        // Expected win/loss ratio (simplified)
        let win_loss_ratio = 2.0; // Assume 2:1 reward:risk ratio
        
        // Kelly formula: f* = (bp - q) / b = (win_probability * win_loss_ratio - (1 - win_probability)) / win_loss_ratio
        let kelly_fraction = (win_probability * win_loss_ratio - (1.0 - win_probability)) / win_loss_ratio;
        
        // Prevent extreme position sizes and apply a safety factor of 0.5 (half-Kelly)
        let safe_kelly = (kelly_fraction * 0.5).max(0.0).min(0.2);
        
        // Calculate position size based on portfolio value
        let position_size = self.portfolio_value * safe_kelly;
        
        // Cap at maximum position size
        position_size.min(max_position)
    }
    
    /// Evaluate whether to enter a trade based on advanced criteria
    pub async fn evaluate_entry(
        &mut self,
        token_mint: &str,
        token_info: &ExtendedTokenInfo,
        token_tracker: &TokenTracker,
        max_position_size: f64,
    ) -> (bool, f64) {
        // Calculate entry criteria
        let entry_criteria = self.calculate_entry_criteria(token_info, token_tracker).await;
        
        // Store entry criteria for later reference
        self.entry_criteria.insert(token_mint.to_string(), entry_criteria.clone());
        
        // Check if trading is active (not paused by circuit breakers)
        if !self.trading_active {
            self.logger.log(format!("Trading is currently paused due to circuit breakers - skipping {}", token_mint).red().to_string());
            return (false, 0.0);
        }
        
        // Check for buy signal
        if !entry_criteria.has_buy_signal {
            return (false, 0.0);
        }
        
        // Calculate position size using Kelly criterion
        let position_size = self.calculate_kelly_position(
            token_mint, 
            entry_criteria.entry_confidence,
            max_position_size
        );
        
        // Log the entry decision
        self.logger.log(format!(
            "ENTRY SIGNAL: {} - Confidence: {}%, Risk Profile: {:?}, Position Size: {:0.3} SOL",
            token_mint,
            entry_criteria.entry_confidence,
            entry_criteria.risk_profile,
            position_size
        ).green().to_string());
        
        (true, position_size)
    }
    
    /// Update exit strategy based on current market conditions and price action
    pub fn update_exit_strategy(
        &mut self,
        token_mint: &str,
        current_price: f64,
        max_price: f64,
        buy_price: f64,
        time_elapsed: Duration,
    ) -> Option<(bool, u64, String)> {
        let strategy = self.exit_strategies.get_mut(token_mint)?;
        
        // Calculate current PNL
        let current_pnl = ((current_price - buy_price) / buy_price) * 100.0;
        
        // Calculate drawdown from peak
        let drawdown = if max_price > current_price {
            ((max_price - current_price) / max_price) * 100.0
        } else {
            0.0
        };
        
        strategy.current_drawdown = drawdown;
        
        // Check for trailing stop hit
        if current_pnl > strategy.initial_take_profit_pct * 0.5 { // Only enable trailing stop after 50% of take profit target
            let trailing_stop_price = max_price * (1.0 - strategy.trailing_stop_distance / 100.0);
            
            if current_price <= trailing_stop_price {
                return Some((
                    true, 
                    100, // Sell 100% on trailing stop hit
                    format!("Trailing stop triggered at {:0.2}% drawdown from peak", drawdown)
                ));
            }
        }
        
        // Check take profit levels
        for (tp_level, sell_percentage) in &strategy.take_profit_levels {
            if current_pnl >= *tp_level {
                return Some((
                    true, 
                    (sell_percentage * 100.0) as u64, 
                    format!("Take profit level hit: {:0.2}%", tp_level)
                ));
            }
        }
        
        // Check time-based exits
        let elapsed_seconds = time_elapsed.as_secs();
        for (time_threshold, sell_percentage) in &strategy.time_based_exits {
            if elapsed_seconds > *time_threshold {
                return Some((
                    true, 
                    (sell_percentage * 100.0) as u64, 
                    format!("Time-based exit after {} seconds", time_threshold)
                ));
            }
        }
        
        // Check risk-adjusted stop loss
        if current_pnl <= -strategy.risk_adjusted_stop_loss {
            return Some((
                true, 
                100, // Sell 100% on stop loss hit
                format!("Stop loss triggered at {:0.2}%", strategy.risk_adjusted_stop_loss)
            ));
        }
        
        // No exit signal
        None
    }
    
    /// Evaluate market sentiment based on overall token performance
    pub fn update_market_sentiment(&mut self, token_data: &[ExtendedTokenInfo]) {
        if token_data.is_empty() {
            return;
        }
        
        // Calculate average buy/sell ratio across tokens
        let avg_buy_sell_ratio = token_data.iter()
            .map(|token| {
                if token.sell_tx_num == 0 {
                    token.buy_tx_num as f64
                } else {
                    token.buy_tx_num as f64 / token.sell_tx_num as f64
                }
            })
            .sum::<f64>() / token_data.len() as f64;
            
        // Calculate average price change
        let avg_price_change = token_data.iter()
            .filter_map(|token| {
                if token.max_token_price > 0.0 {
                    Some(token.current_token_price / token.max_token_price)
                } else {
                    None
                }
            })
            .sum::<f64>() / token_data.len() as f64;
            
        // Update market sentiment (0-100 scale)
        let sentiment_score = {
            // Base score from buy/sell ratio (33% weight)
            let buy_sell_component = (avg_buy_sell_ratio.min(3.0) / 3.0) * 33.0;
            
            // Price momentum component (33% weight)
            let price_momentum = (avg_price_change.min(2.0) / 2.0) * 33.0;
            
            // Remaining 34% from prior sentiment with slight mean reversion
            let prior_sentiment_component = (50.0 + (self.market_sentiment - 50.0) * 0.8) * 0.34;
            
            buy_sell_component + price_momentum + prior_sentiment_component
        };
        
        self.market_sentiment = sentiment_score.clamp(0.0, 100.0);
    }
    
    /// Check circuit breakers and potentially pause trading
    pub fn check_circuit_breakers(&mut self) {
        // Check if daily loss limit has been hit
        if self.current_day_pnl <= -self.daily_loss_limit {
            self.trading_active = false;
            self.logger.log(
                format!("CIRCUIT BREAKER TRIGGERED: Daily loss limit of {}% has been hit. Trading paused.", self.daily_loss_limit)
                .red().bold().to_string()
            );
        }
        
        // Check market sentiment for extreme bearishness
        if self.market_sentiment < 20.0 {
            self.trading_active = false;
            self.logger.log(
                format!("CIRCUIT BREAKER TRIGGERED: Market sentiment extremely bearish ({}). Trading paused.", self.market_sentiment)
                .red().bold().to_string()
            );
        }
    }
    
    /// Record trade performance for self-tuning
    pub fn record_trade_performance(&mut self, token_mint: &str, entry_price: f64, exit_price: f64, position_size: f64) {
        // Calculate percentage gain/loss
        let pnl_percent = ((exit_price - entry_price) / entry_price) * 100.0;
        
        // Update performance metrics
        self.performance_metrics
            .entry(token_mint.to_string())
            .or_insert_with(Vec::new)
            .push(pnl_percent);
            
        // Update daily P&L
        let pnl_contribution = pnl_percent * (position_size / self.portfolio_value);
        self.current_day_pnl += pnl_contribution;
        
        // Check circuit breakers after trade
        self.check_circuit_breakers();
    }
    
    /// Reset daily P&L counter (call at beginning of trading day)
    pub fn reset_daily_pnl(&mut self) {
        self.current_day_pnl = 0.0;
        self.trading_active = true;
    }
    
    /// Update portfolio value
    pub fn update_portfolio_value(&mut self, new_value: f64) {
        self.portfolio_value = new_value;
    }
}

/// Integration functions for the advanced trading system
pub mod integration {
    use super::*;
    
    /// Start the advanced trading system monitoring loop
    pub async fn start_advanced_trading_system(
        token_tracker: Arc<TokenTracker>,
        _sell_manager: Arc<Mutex<SellManager>>,
        _swapx: Pump,
        _existing_liquidity_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
        _telegram_service: Option<Arc<TelegramService>>,
        logger: Logger,
        portfolio_value: f64,
    ) -> Result<()> {
        let advanced_trading = Arc::new(Mutex::new(
            AdvancedTradingManager::new(logger.clone())
        ));
        
        // Update initial portfolio value
        advanced_trading.lock().unwrap().update_portfolio_value(portfolio_value);
        
        // Start market sentiment update loop
        let trading_clone = advanced_trading.clone();
        let token_tracker_clone = token_tracker.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300)); // Update every 5 minutes
            
            loop {
                interval.tick().await;
                
                // Get all tracked tokens
                let extended_tokens = token_tracker_clone.get_extended_tokens_map_arc();
                let tokens_map = extended_tokens.lock().unwrap();
                let token_data: Vec<ExtendedTokenInfo> = tokens_map
                    .values()
                    .map(|token| token.clone_without_task())
                    .collect();
                
                // Drop the lock
                drop(tokens_map);
                
                // Update market sentiment
                if let Ok(mut trading_system) = trading_clone.lock() {
                    trading_system.update_market_sentiment(&token_data);
                    
                    // Log current market sentiment
                    trading_system.logger.log(
                        format!("Market sentiment updated: {:.1}/100", trading_system.market_sentiment)
                        .cyan().to_string()
                    );
                    
                    // Check circuit breakers
                    trading_system.check_circuit_breakers();
                }
            }
        });
        
        // Integration with token_tracker and sell_manager would be implemented here
        
        Ok(())
    }
    
    /// Evaluate a token for potential entry
    pub async fn evaluate_token_entry(
        trading_manager: &Arc<Mutex<AdvancedTradingManager>>,
        token_tracker: &Arc<TokenTracker>,
        token_mint: &str,
        max_position_size: f64,
    ) -> (bool, f64) {
        // Get token information
        let extended_tokens = token_tracker.get_extended_tokens_map_arc();
        let tokens_map = extended_tokens.lock().unwrap();
        
        if let Some(token_info) = tokens_map.get(token_mint) {
            let token_info = token_info.clone_without_task();
            
            // Drop lock before async call
            drop(tokens_map);
            
            // Get trading manager lock
            if let Ok(mut trading_manager) = trading_manager.lock() {
                return trading_manager.evaluate_entry(
                    token_mint,
                    &token_info,
                    token_tracker,
                    max_position_size
                ).await;
            }
        } else {
            // Token not found
            drop(tokens_map);
        }
        
        (false, 0.0)
    }
    
    /// Create an exit strategy for a token
    pub fn create_token_exit_strategy(
        trading_manager: &Arc<Mutex<AdvancedTradingManager>>,
        token_mint: &str,
        risk_profile: RiskProfile,
        buy_price: f64,
    ) -> Option<DynamicExitStrategy> {
        if let Ok(mut trading_manager) = trading_manager.lock() {
            let strategy = trading_manager.create_exit_strategy(token_mint, &risk_profile, buy_price);
            trading_manager.exit_strategies.insert(token_mint.to_string(), strategy.clone());
            Some(strategy)
        } else {
            None
        }
    }
    
    /// Check exit conditions for a token
    pub fn check_token_exit(
        trading_manager: &Arc<Mutex<AdvancedTradingManager>>,
        token_mint: &str,
        current_price: f64,
        max_price: f64,
        buy_price: f64,
        time_elapsed: Duration,
    ) -> Option<(bool, u64, String)> {
        if let Ok(mut trading_manager) = trading_manager.lock() {
            trading_manager.update_exit_strategy(
                token_mint,
                current_price,
                max_price,
                buy_price,
                time_elapsed
            )
        } else {
            None
        }
    }
} 