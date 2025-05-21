use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::{Result, anyhow};
use colored::Colorize;
use tokio::time;
use tokio::time::Instant;

use crate::common::logger::Logger;
use crate::dex::pump_fun::Pump;
use crate::engine::swap::{SwapDirection, SwapInType};
use crate::common::config::{SwapConfig, Status, LiquidityPool};
use crate::core::tx;
use crate::services::telegram::TelegramService;
use crate::engine::token_tracker::{TokenTracker, ExtendedTokenInfo};
use crate::engine::advanced_trading::{RiskProfile, AdvancedTradingManager};
use crate::engine::monitor::BondingCurveInfo;

/// Defines a strategy for token buying with configurable parameters
#[derive(Clone, Debug)]
pub struct BuyStrategy {
    /// Mint address of the token
    pub token_mint: String,
    /// Current price of the token
    pub current_price: f64,
    /// Amount to buy in SOL
    pub buy_amount: f64,
    /// Maximum price increase tolerated for entry (%)
    pub max_entry_slippage: f64,
    /// Risk profile of the token
    pub risk_profile: RiskProfile,
    /// Minimum buy-to-sell ratio to consider purchase
    pub min_buy_sell_ratio: f64,
    /// Maximum market cap to consider (in SOL)
    pub max_market_cap: f64,
    /// Minimum launcher SOL balance
    pub min_launcher_sol: f64,
    /// Maximum launcher SOL balance
    pub max_launcher_sol: f64,
    /// Whether this is a time-sensitive opportunity
    pub time_sensitive: bool,
    /// Confidence score (0-100)
    pub confidence: u64,
    /// Timestamp when the buy strategy was created
    pub created_at: Instant,
    /// Maximum age of token to consider for buying (in seconds)
    pub max_token_age: u64,
    /// Whether buying is enabled for this token
    pub buying_enabled: bool,
}

impl BuyStrategy {
    /// Create a new BuyStrategy with default parameters
    pub fn new(token_mint: String, current_price: f64) -> Self {
        // Load strategy parameters from environment or use defaults
        let buy_amount = std::env::var("BUY_AMOUNT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.1);  // Default 0.1 SOL
            
        let max_entry_slippage = std::env::var("MAX_ENTRY_SLIPPAGE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0);  // Default 5% max entry slippage
            
        let max_token_age = std::env::var("MAX_TOKEN_AGE_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);  // Default 5 minutes (300 seconds)
            
        Self {
            token_mint,
            current_price,
            buy_amount,
            max_entry_slippage,
            risk_profile: RiskProfile::Medium,
            min_buy_sell_ratio: 1.2,
            max_market_cap: 500.0, // 500 SOL max market cap
            min_launcher_sol: 0.5,  // Minimum 0.5 SOL for launcher
            max_launcher_sol: 50.0, // Maximum 50 SOL for launcher
            time_sensitive: false,
            confidence: 50, // Default medium confidence
            created_at: Instant::now(),
            max_token_age,
            buying_enabled: true,
        }
    }
    
    /// Check if a token is suitable for buying based on token information
    pub fn is_suitable_for_buying(&self, token_info: &ExtendedTokenInfo) -> bool {
        // Check if buying is enabled for this token
        if !self.buying_enabled {
            return false;
        }
        
        // Skip tokens that are too old
        let token_age = token_info.token_mint_timestamp.elapsed().as_secs();
        if token_age > self.max_token_age {
            return false;
        }
        
        // Check market cap
        if let Some(market_cap) = token_info.market_cap {
            if market_cap > self.max_market_cap {
                return false;
            }
        }
        
        // Check launcher SOL balance
        if let Some(launcher_sol) = token_info.launcher_sol_balance {
            if launcher_sol < self.min_launcher_sol || launcher_sol > self.max_launcher_sol {
                return false;
            }
        }
        
        // Check buy/sell ratio
        let buy_sell_ratio = if token_info.sell_tx_num == 0 {
            token_info.buy_tx_num as f64
        } else {
            token_info.buy_tx_num as f64 / token_info.sell_tx_num as f64
        };
        
        if buy_sell_ratio < self.min_buy_sell_ratio {
            return false;
        }
        
        // All checks passed
        true
    }
    
    /// Calculate the buy amount based on risk profile
    pub fn calculate_buy_amount(&self) -> f64 {
        match self.risk_profile {
            RiskProfile::Low => self.buy_amount * 1.5, // Higher allocation for low risk
            RiskProfile::Medium => self.buy_amount,
            RiskProfile::High => self.buy_amount * 0.7, // Lower allocation for high risk
            RiskProfile::VeryHigh => self.buy_amount * 0.5, // Lowest allocation for very high risk
        }
    }
    
    /// Check if token price has increased beyond acceptable entry point
    pub fn is_price_acceptable(&self, current_price: f64) -> bool {
        let initial_price = self.current_price;
        
        // If the price has increased more than max_entry_slippage percentage, reject the entry
        let price_increase_pct = ((current_price - initial_price) / initial_price) * 100.0;
        price_increase_pct <= self.max_entry_slippage
    }
    
    /// Set the risk profile based on token metrics
    pub fn set_risk_profile(&mut self, risk_profile: RiskProfile) {
        self.risk_profile = risk_profile;
    }
    
    /// Adjust confidence score based on new information
    pub fn adjust_confidence(&mut self, new_confidence: u64) {
        self.confidence = new_confidence.min(100); // Cap at 100
    }
}

/// Manager for token buying operations
#[derive(Clone)]
pub struct BuyManager {
    /// Buy strategies for tokens
    strategies: HashMap<String, BuyStrategy>,
    /// Set of tokens that have been bought already to avoid duplicate buys
    bought_tokens: HashSet<String>,
    /// Logger for buy operations
    logger: Logger,
    /// Maximum SOL to spend in a single day
    daily_budget: f64,
    /// SOL spent so far today
    daily_spent: f64,
    /// Last budget reset timestamp
    last_budget_reset: Instant,
    /// Whether the buy manager is active
    active: bool,
}

impl BuyManager {
    /// Create a new BuyManager
    pub fn new(logger: Logger) -> Self {
        // Load daily budget from environment or use default
        let daily_budget = std::env::var("DAILY_BUY_BUDGET")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0);  // Default 1 SOL daily budget
            
        Self {
            strategies: HashMap::new(),
            bought_tokens: HashSet::new(),
            logger,
            daily_budget,
            daily_spent: 0.0,
            last_budget_reset: Instant::now(),
            active: true,
        }
    }
    
    /// Add a token to the buy strategies
    pub fn add_token_strategy(&mut self, token_mint: String, current_price: f64) -> Result<()> {
        // Skip if token is already added or already bought
        if self.strategies.contains_key(&token_mint) || self.bought_tokens.contains(&token_mint) {
            return Err(anyhow!("Token already in strategies or already bought"));
        }
        
        let strategy = BuyStrategy::new(token_mint.clone(), current_price);
        self.strategies.insert(token_mint, strategy);
        
        Ok(())
    }
    
    /// Remove a token strategy
    pub fn remove_token_strategy(&mut self, token_mint: &str) -> Option<BuyStrategy> {
        self.strategies.remove(token_mint)
    }
    
    /// Mark a token as bought to avoid duplicate buys
    pub fn mark_token_as_bought(&mut self, token_mint: &str) {
        self.bought_tokens.insert(token_mint.to_string());
        self.strategies.remove(token_mint);
    }
    
    /// Update price for a token strategy
    pub fn update_token_price(&mut self, token_mint: &str, new_price: f64) -> Result<()> {
        if let Some(strategy) = self.strategies.get_mut(token_mint) {
            strategy.current_price = new_price;
            Ok(())
        } else {
            Err(anyhow!("Token strategy not found"))
        }
    }
    
    /// Check if a token has been bought already
    pub fn is_token_bought(&self, token_mint: &str) -> bool {
        self.bought_tokens.contains(token_mint)
    }
    
    /// Set risk profile for a token
    pub fn set_token_risk_profile(&mut self, token_mint: &str, risk_profile: RiskProfile) -> Result<()> {
        if let Some(strategy) = self.strategies.get_mut(token_mint) {
            strategy.set_risk_profile(risk_profile);
            Ok(())
        } else {
            Err(anyhow!("Token strategy not found"))
        }
    }
    
    /// Reset the daily budget at midnight
    pub fn check_and_reset_daily_budget(&mut self) {
        // Reset daily budget after 24 hours
        if self.last_budget_reset.elapsed() > Duration::from_secs(24 * 60 * 60) {
            self.daily_spent = 0.0;
            self.last_budget_reset = Instant::now();
            self.logger.log("Daily buy budget has been reset".yellow().to_string());
        }
    }
    
    /// Check if there's enough budget for a purchase
    pub fn has_budget_for_purchase(&self, amount: f64) -> bool {
        (self.daily_spent + amount) <= self.daily_budget
    }
    
    /// Update budget after a purchase
    pub fn update_budget_after_purchase(&mut self, amount: f64) {
        self.daily_spent += amount;
        self.logger.log(format!(
            "Budget update: Spent {:.3} SOL, Remaining {:.3} SOL for today",
            amount,
            self.daily_budget - self.daily_spent
        ).yellow().to_string());
    }
    
    /// Set active state
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        self.logger.log(format!(
            "Buy manager is now {}",
            if active { "active".green() } else { "inactive".red() }
        ).to_string());
    }
    
    /// Execute buy for a token if conditions are met
    pub async fn execute_buy(
        &mut self,
        token_mint: &str,
        swapx: &Pump,
        swap_config: &SwapConfig,
        existing_liquidity_pools: &Arc<Mutex<HashSet<LiquidityPool>>>,
        telegram_service: &Option<Arc<TelegramService>>,
        telegram_chat_id: &str,
        token_info: &ExtendedTokenInfo,
        bonding_curve_info: Option<BondingCurveInfo>,
    ) -> Result<bool> {
        // Skip if not active
        if !self.active {
            return Ok(false);
        }
        
        // Check budget first
        self.check_and_reset_daily_budget();
        
        // Get strategy for this token
        let strategy = match self.strategies.get(token_mint) {
            Some(s) => s.clone(),
            None => return Err(anyhow!("No buy strategy for this token")),
        };
        
        // Skip if already bought
        if self.is_token_bought(token_mint) {
            return Ok(false);
        }
        
        // Check if token is suitable for buying
        if !strategy.is_suitable_for_buying(token_info) {
            return Ok(false);
        }
        
        // Calculate buy amount based on risk profile
        let buy_amount = strategy.calculate_buy_amount();
        
        // Check if we have budget for this purchase
        if !self.has_budget_for_purchase(buy_amount) {
            self.logger.log(format!(
                "Skipping buy for {} - Insufficient daily budget (Need {:.3} SOL, Available {:.3} SOL)",
                token_mint,
                buy_amount,
                self.daily_budget - self.daily_spent
            ).red().to_string());
            return Ok(false);
        }
        
        // Check if price is still acceptable
        if !strategy.is_price_acceptable(token_info.current_token_price) {
            self.logger.log(format!(
                "Skipping buy for {} - Price increased too much (Initial: {:.8}, Current: {:.8})",
                token_mint,
                strategy.current_price,
                token_info.current_token_price
            ).red().to_string());
            return Ok(false);
        }
        
        // Log buy intent
        self.logger.log(format!(
            "🟩 BUY ORDER: Buying {} SOL worth of {} at price {}",
            buy_amount,
            token_mint,
            token_info.current_token_price
        ).green().to_string());
        
        // Send telegram notification
        if let Some(telegram) = telegram_service {
            let message = format!(
                "🟩 Buy Order Initiated:\n`{}`\n💰 Amount: {} SOL\n💲 Price: {} SOL\n📊 Market Cap: {} SOL\n👥 Buy/Sell Ratio: {}",
                token_mint,
                buy_amount,
                token_info.current_token_price,
                token_info.market_cap.unwrap_or(0.0),
                if token_info.sell_tx_num == 0 { token_info.buy_tx_num.to_string() } else { format!("{:.2}", token_info.buy_tx_num as f64 / token_info.sell_tx_num as f64) }
            );
            
            // Try to send notification
            if let Err(e) = telegram.send_message(telegram_chat_id, &message, "Markdown").await {
                self.logger.log(format!("Failed to send Telegram notification: {}", e).red().to_string());
            }
        }
        
        // Execute the buy
        let start_time = Instant::now();
        
        // Create swap config with buy direction
        let custom_swap_config = SwapConfig {
            swap_direction: SwapDirection::Buy,
            in_type: SwapInType::Qty,
            amount_in: buy_amount,
            slippage: swap_config.slippage,
            use_jito: swap_config.use_jito,
        };
        
        // Use build_swap_ixn_by_mint to create the transaction
        match swapx.build_swap_ixn_by_mint(
            token_mint,
            bonding_curve_info,
            custom_swap_config.clone(),
            start_time,
        ).await {
            Ok(result) => {
                let (keypair, instructions, token_price) = result;
                
                self.logger.log(format!(
                    "Built buy order for {} SOL of {} at price {} SOL",
                    buy_amount,
                    token_mint,
                    token_price
                ).yellow().to_string());
                
                // Get latest blockhash
                if let Ok(recent_blockhash) = swapx.rpc_nonblocking_client.get_latest_blockhash().await {
                    // Send the transaction
                    match tx::new_signed_and_send_nozomi(
                        recent_blockhash,
                        &keypair,
                        instructions,
                        &self.logger,
                    ).await {
                        Ok(res) => {
                            self.logger.log(format!(
                                "✅ Executed buy order: {} SOL of {} - TX: {}",
                                buy_amount,
                                token_mint,
                                res[0]
                            ).green().to_string());
                            
                            // Mark token as bought
                            self.mark_token_as_bought(token_mint);
                            
                            // Update budget
                            self.update_budget_after_purchase(buy_amount);
                            
                            // Update the pool status in liquidityPools
                            let mint_for_update = token_mint.to_string();
                            let token_price_for_update = token_price;
                            
                            // Use a separate scope for the mutex lock
                            {
                                let mut pools = existing_liquidity_pools.lock().unwrap();
                                
                                // Create a new pool entry
                                let new_pool = LiquidityPool {
                                    mint: mint_for_update.clone(),
                                    status: Status::Bought,
                                    buy_price: token_price_for_update,
                                    sell_price: 0.0,
                                    timestamp: Some(Instant::now()),
                                };
                                
                                pools.insert(new_pool);
                            }
                            
                            // Send confirmation via telegram
                            if let Some(telegram) = telegram_service {
                                // Use the new transaction notification method
                                if let Err(e) = telegram.send_transaction_notification(
                                    "buy",
                                    token_mint,
                                    token_info.token_symbol.as_deref(),
                                    buy_amount,
                                    token_info.token_amount.unwrap_or(0.0),
                                    token_price,
                                    &res[0],
                                    None,
                                ).await {
                                    self.logger.log(format!("Failed to send success notification: {}", e).red().to_string());
                                }
                            }
                            
                            return Ok(true);
                        },
                        Err(e) => {
                            self.logger.log(format!(
                                "❌ Failed to execute buy order for {}: {}",
                                token_mint,
                                e
                            ).red().to_string());
                            
                            // Send failure notification
                            if let Some(telegram) = telegram_service {
                                let failure_message = format!(
                                    "❌ Buy Order Failed:\n`{}`\n💰 Amount: {} SOL\n💲 Price: {} SOL\n🛑 Error: {}",
                                    token_mint,
                                    buy_amount,
                                    token_price,
                                    e
                                );
                                
                                if let Err(e) = telegram.send_message(telegram_chat_id, &failure_message, "Markdown").await {
                                    self.logger.log(format!("Failed to send failure notification: {}", e).red().to_string());
                                }
                            }
                        }
                    }
                } else {
                    self.logger.log("Failed to get recent blockhash for buy transaction".red().to_string());
                }
            },
            Err(e) => {
                self.logger.log(format!(
                    "❌ Failed to build buy order for {}: {}",
                    token_mint,
                    e
                ).red().to_string());
                
                // Send failure notification
                if let Some(telegram) = telegram_service {
                    let build_failure_message = format!(
                        "❌ Failed to Build Buy Order:\n`{}`\n💰 Amount: {} SOL\n🛑 Error: {}",
                        token_mint,
                        buy_amount,
                        e
                    );
                    
                    if let Err(e) = telegram.send_message(telegram_chat_id, &build_failure_message, "Markdown").await {
                        self.logger.log(format!("Failed to send build failure notification: {}", e).red().to_string());
                    }
                }
            }
        }
        
        Ok(false)
    }
    
    /// Start background monitoring for buy opportunities
    pub async fn start_monitoring_loop(
        mut self,
        token_tracker: Arc<TokenTracker>,
        swapx: Pump,
        swap_config: SwapConfig,
        existing_liquidity_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
        telegram_service: Option<Arc<TelegramService>>,
        telegram_chat_id: String,
        advanced_trading_manager: Option<Arc<Mutex<AdvancedTradingManager>>>,
    ) {
        let monitoring_interval = Duration::from_secs(2);
        let mut interval = time::interval(monitoring_interval);
        
        self.logger.log("Starting buy manager monitoring loop".blue().to_string());
        
        loop {
            interval.tick().await;
            
            // Check and reset daily budget if needed
            self.check_and_reset_daily_budget();
            
            // Get current tracked tokens
            let extended_tokens_map = token_tracker.get_extended_tokens_map_arc();
            let extended_tokens = extended_tokens_map.lock().unwrap();
            
            // Process tokens for potential buys
            for (token_mint, token_info) in extended_tokens.iter() {
                // Skip already bought tokens
                if self.is_token_bought(token_mint) {
                    continue;
                }
                
                // Add to strategies if not already present
                if !self.strategies.contains_key(token_mint) {
                    let _ = self.add_token_strategy(token_mint.clone(), token_info.current_token_price);
                    
                    // Set risk profile based on token metrics
                    if let Some(trading_manager) = &advanced_trading_manager {
                        let buy_sell_ratio = if token_info.sell_tx_num == 0 {
                            token_info.buy_tx_num as f64
                        } else {
                            token_info.buy_tx_num as f64 / token_info.sell_tx_num as f64
                        };
                        
                        let locked_manager = trading_manager.lock().unwrap();
                        let risk_profile = locked_manager.calculate_risk_profile(
                            token_info.market_cap.unwrap_or(0.0),
                            token_info.volume.unwrap_or(0.0),
                            buy_sell_ratio,
                            token_info.token_mint_timestamp.elapsed().as_secs(),
                            token_info.launcher_sol_balance.unwrap_or(0.0),
                        );
                        
                        let _ = self.set_token_risk_profile(token_mint, risk_profile);
                    }
                }
                
                // Update price in strategy
                let _ = self.update_token_price(token_mint, token_info.current_token_price);
                
                // Try to execute buy
                // Note: No bonding curve info provided here, the function will fetch it if needed
                if let Err(e) = self.execute_buy(
                    token_mint,
                    &swapx,
                    &swap_config,
                    &existing_liquidity_pools,
                    &telegram_service,
                    &telegram_chat_id,
                    token_info,
                    None,
                ).await {
                    self.logger.log(format!("Error executing buy for {}: {}", token_mint, e).red().to_string());
                }
            }
            
            // Clean up old strategies
            let current_time = Instant::now();
            let strategies_to_remove: Vec<String> = self.strategies.iter()
                .filter(|(_, strategy)| {
                    current_time.duration_since(strategy.created_at) > Duration::from_secs(strategy.max_token_age)
                })
                .map(|(token_mint, _)| token_mint.clone())
                .collect();
            
            for token_mint in strategies_to_remove {
                self.strategies.remove(&token_mint);
            }
        }
    }
    
    /// Update the buy amount for a specific token
    pub fn update_token_buy_amount(&mut self, token_mint: &str, amount: f64) -> Result<()> {
        if let Some(strategy) = self.strategies.get_mut(token_mint) {
            strategy.buy_amount = amount;
            Ok(())
        } else {
            Err(anyhow!("Token strategy not found"))
        }
    }
} 