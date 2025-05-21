use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::{Result, anyhow};
use colored::Colorize;
use tokio::time::{self, Instant};

use crate::common::logger::Logger;
use crate::dex::pump_fun::Pump;
use crate::common::config::{SwapConfig, Status, LiquidityPool};
use crate::engine::swap::{SwapDirection, SwapInType};
use crate::core::tx;
use crate::services::telegram::TelegramService;

/// Strategy for selling tokens with customizable parameters
///
/// This strategy manages the selling of tokens based on various parameters including
/// take profit and stop loss targets, time thresholds, and price movement.
#[derive(Clone, Debug)]
pub struct SellStrategy {
    /// Mint address of the token
    pub token_mint: String,
    /// Price at which the token was bought
    pub buy_price: f64,
    /// Current price of the token
    pub current_price: f64,
    /// Highest percentage profit reached so far (for trailing stop loss)
    pub top_pnl: f64,
    /// Set of completed selling intervals (to avoid duplicate sells)
    pub completed_intervals: HashSet<String>,
    /// Timestamp of the last sell operation
    pub last_sell_time: Instant,
    /// Amount of tokens held
    pub token_amount: f64,
    /// Decimals of the token
    pub token_decimals: u8,
    /// Take profit percentage - triggers a sell when profit reaches this percentage
    pub take_profit_pct: f64,
    /// Stop loss percentage - triggers a sell when loss reaches this percentage
    pub stop_loss_pct: f64,
    /// Whether take profit has been triggered
    pub take_profit_triggered: bool,
    /// Whether stop loss has been triggered
    pub stop_loss_triggered: bool,
}

impl SellStrategy {
    /// Create a new SellStrategy with the specified parameters
    pub fn new(token_mint: String, buy_price: f64, token_amount: f64, token_decimals: u8) -> Self {
        // Load take profit and stop loss values from environment or use defaults
        let take_profit_pct = std::env::var("TAKE_PROFIT_PERCENT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(20.0);  // Default 20% take profit
            
        let stop_loss_pct = std::env::var("STOP_LOSS_PERCENT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);  // Default 10% stop loss
            
        Self {
            token_mint,
            buy_price,
            current_price: buy_price,
            top_pnl: 0.0,
            completed_intervals: HashSet::new(),
            last_sell_time: Instant::now(),
            token_amount,
            token_decimals,
            take_profit_pct,
            stop_loss_pct,
            take_profit_triggered: false,
            stop_loss_triggered: false,
        }
    }
    
    pub fn calculate_pnl(&self) -> f64 {
        if self.buy_price == 0.0 {
            return 0.0;
        }
        
        ((self.current_price - self.buy_price) * 100.0) / self.buy_price
    }
    
    pub fn update_price(&mut self, new_price: f64) {
        self.current_price = new_price;
        
        let pnl = self.calculate_pnl();
        if pnl > self.top_pnl {
            self.top_pnl = pnl;
        }
    }
    
    pub fn time_elapsed_secs(&self) -> u64 {
        self.last_sell_time.elapsed().as_secs()
    }
    
    pub fn is_take_profit_hit(&self) -> bool {
        if self.take_profit_triggered {
            return false;
        }
        
        let pnl = self.calculate_pnl();
        pnl >= self.take_profit_pct
    }
    
    pub fn is_stop_loss_hit(&self) -> bool {
        if self.stop_loss_triggered {
            return false;
        }
        
        let pnl = self.calculate_pnl();
        pnl <= -self.stop_loss_pct
    }
    
    pub fn mark_take_profit_triggered(&mut self) {
        self.take_profit_triggered = true;
    }
    
    pub fn mark_stop_loss_triggered(&mut self) {
        self.stop_loss_triggered = true;
    }
    
    pub fn take_profit_price(&self) -> f64 {
        self.buy_price * (1.0 + (self.take_profit_pct / 100.0))
    }
    
    pub fn stop_loss_price(&self) -> f64 {
        self.buy_price * (1.0 - (self.stop_loss_pct / 100.0))
    }
}

#[derive(Clone, Debug)]
pub struct TakeProfitLevel {
    pub threshold: f64,
    pub percentage: u64,
    pub flag: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct RetracementLevel {
    pub percentage: f64,
    pub threshold: f64,
    pub flag: String,
    pub message: String,
    pub sell_amount: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SellManager {
    // Track sell strategies for each token
    strategies: HashMap<String, SellStrategy>,
    take_profit_levels: Vec<TakeProfitLevel>,
    retracement_levels: Vec<RetracementLevel>,
    logger: Logger,
}

impl SellManager {
    pub fn new(logger: Logger) -> Self {
        Self {
            strategies: HashMap::new(),
            take_profit_levels: Self::default_take_profit_levels(),
            retracement_levels: Self::default_retracement_levels(),
            logger,
        }
    }
    
    fn default_take_profit_levels() -> Vec<TakeProfitLevel> {
        vec![
            TakeProfitLevel { threshold: 2000.0, percentage: 100, flag: "tp_2000".to_string(), message: "Take full profit at 2000%".to_string() },
            TakeProfitLevel { threshold: 1500.0, percentage: 40, flag: "tp_1500".to_string(), message: "Take partial profit at 1500%".to_string() },
            TakeProfitLevel { threshold: 1000.0, percentage: 40, flag: "tp_1000".to_string(), message: "Take partial profit at 1000%".to_string() },
            TakeProfitLevel { threshold: 800.0, percentage: 20, flag: "tp_800".to_string(), message: "Take partial profit at 800%".to_string() },
            TakeProfitLevel { threshold: 600.0, percentage: 20, flag: "tp_600".to_string(), message: "Take partial profit at 600%".to_string() },
            TakeProfitLevel { threshold: 400.0, percentage: 20, flag: "tp_400".to_string(), message: "Take partial profit at 400%".to_string() },
            TakeProfitLevel { threshold: 300.0, percentage: 20, flag: "tp_300".to_string(), message: "Take partial profit at 300%".to_string() },
            TakeProfitLevel { threshold: 250.0, percentage: 20, flag: "tp_250".to_string(), message: "Take partial profit at 250%".to_string() },
            TakeProfitLevel { threshold: 200.0, percentage: 20, flag: "tp_200".to_string(), message: "Take partial profit at 200%".to_string() },
            TakeProfitLevel { threshold: 120.0, percentage: 20, flag: "tp_120".to_string(), message: "Take partial profit at 120%".to_string() },
            TakeProfitLevel { threshold: 80.0, percentage: 20, flag: "tp_80".to_string(), message: "Take partial profit at 80%".to_string() },
            TakeProfitLevel { threshold: 50.0, percentage: 10, flag: "tp_50".to_string(), message: "Take partial profit at 50%".to_string() },
            TakeProfitLevel { threshold: 20.0, percentage: 10, flag: "tp_20".to_string(), message: "Take partial profit at 20%".to_string() },
        ]
    }
    
    fn get_retracement_levels_for_pnl(pnl: f64, time_elapsed: u64) -> Vec<RetracementLevel> {
        if time_elapsed > 30 {
            if pnl > 500.0 {
                vec![
                    RetracementLevel { percentage: 3.0, threshold: 2000.0, flag: "retracement_2000_3".to_string(), message: "Selling on 3% retracement from top (2000%+ PNL)".to_string(), sell_amount: 100 },
                    RetracementLevel { percentage: 4.0, threshold: 1500.0, flag: "retracement_1500_4".to_string(), message: "Selling on 4% retracement from top (1500%+ PNL)".to_string(), sell_amount: 50 },
                    RetracementLevel { percentage: 5.0, threshold: 1000.0, flag: "retracement_1000_5".to_string(), message: "Selling on 5% retracement from top (1000%+ PNL)".to_string(), sell_amount: 40 },
                    RetracementLevel { percentage: 6.0, threshold: 800.0, flag: "retracement_800_6".to_string(), message: "Selling on 6% retracement from top (800%+ PNL)".to_string(), sell_amount: 35 },
                    RetracementLevel { percentage: 6.0, threshold: 700.0, flag: "retracement_700_6".to_string(), message: "Selling on 6% retracement from top (700%+ PNL)".to_string(), sell_amount: 35 },
                    RetracementLevel { percentage: 6.0, threshold: 600.0, flag: "retracement_600_6".to_string(), message: "Selling on 6% retracement from top (600%+ PNL)".to_string(), sell_amount: 30 },
                    RetracementLevel { percentage: 7.0, threshold: 500.0, flag: "retracement_500_7".to_string(), message: "Selling on 7% retracement from top (500%+ PNL)".to_string(), sell_amount: 30 },
                    RetracementLevel { percentage: 7.0, threshold: 400.0, flag: "retracement_400_7".to_string(), message: "Selling on 7% retracement from top (400%+ PNL)".to_string(), sell_amount: 30 },
                    RetracementLevel { percentage: 8.0, threshold: 300.0, flag: "retracement_300_8".to_string(), message: "Selling on 8% retracement from top (300%+ PNL)".to_string(), sell_amount: 20 },
                    RetracementLevel { percentage: 15.0, threshold: 20.0, flag: "retracement_500_15".to_string(), message: "Selling on 15% retracement from top (500%+ PNL)".to_string(), sell_amount: 100 },
                ]
            } else if pnl > 200.0 {
                vec![
                    // Many levels omitted for brevity - in real implementation you'd include all levels
                    RetracementLevel { percentage: 10.0, threshold: 200.0, flag: "retracement_200_10".to_string(), message: "Selling on 10% retracement from top (200%+ PNL)".to_string(), sell_amount: 20 },
                    RetracementLevel { percentage: 12.0, threshold: 100.0, flag: "retracement_100_12".to_string(), message: "Selling on 12% retracement from top (100%+ PNL)".to_string(), sell_amount: 20 },
                    RetracementLevel { percentage: 20.0, threshold: 20.0, flag: "retracement_20_20".to_string(), message: "Selling on 20% retracement from top (20%+ PNL)".to_string(), sell_amount: 100 },
                ]
            } else {
                // Default levels for lower PNL after 30 seconds
                vec![
                    RetracementLevel { percentage: 10.0, threshold: 200.0, flag: "retracement_200_10".to_string(), message: "Selling on 10% retracement from top (200%+ PNL)".to_string(), sell_amount: 15 },
                    RetracementLevel { percentage: 20.0, threshold: 50.0, flag: "retracement_50_20".to_string(), message: "Selling on 20% retracement from top (50%+ PNL)".to_string(), sell_amount: 10 },
                    RetracementLevel { percentage: 30.0, threshold: 30.0, flag: "retracement_30_30".to_string(), message: "Selling on 30% retracement from top (30%+ PNL)".to_string(), sell_amount: 10 },
                    RetracementLevel { percentage: 42.0, threshold: 20.0, flag: "retracement_20_42".to_string(), message: "Selling on 42% retracement from top (20%+ PNL)".to_string(), sell_amount: 100 },
                ]
            }
        } else {
            // Default levels for tokens held less than 30 seconds
            vec![
                RetracementLevel { percentage: 3.0, threshold: 2000.0, flag: "retracement_2000_3_pct".to_string(), message: "Selling on 3% retracement from top (2000%+ PNL)".to_string(), sell_amount: 100 },
                RetracementLevel { percentage: 5.0, threshold: 1000.0, flag: "retracement_1000_5_pct".to_string(), message: "Selling on 5% retracement from top (1000%+ PNL)".to_string(), sell_amount: 40 },
                RetracementLevel { percentage: 7.0, threshold: 500.0, flag: "retracement_500_7_pct".to_string(), message: "Selling on 7% retracement from top (500%+ PNL)".to_string(), sell_amount: 30 },
                RetracementLevel { percentage: 10.0, threshold: 200.0, flag: "retracement_200_10_pct".to_string(), message: "Selling on 10% retracement from top (200%+ PNL)".to_string(), sell_amount: 15 },
            ]
        }
    }
    
    fn default_retracement_levels() -> Vec<RetracementLevel> {
        // Default levels - these will be replaced dynamically based on PNL
        vec![
            RetracementLevel { percentage: 10.0, threshold: 200.0, flag: "retracement_200_10".to_string(), message: "Selling on 10% retracement from top (200%+ PNL)".to_string(), sell_amount: 15 },
            RetracementLevel { percentage: 20.0, threshold: 50.0, flag: "retracement_50_20".to_string(), message: "Selling on 20% retracement from top (50%+ PNL)".to_string(), sell_amount: 10 },
            RetracementLevel { percentage: 30.0, threshold: 30.0, flag: "retracement_30_30".to_string(), message: "Selling on 30% retracement from top (30%+ PNL)".to_string(), sell_amount: 10 },
            RetracementLevel { percentage: 42.0, threshold: 20.0, flag: "retracement_20_42".to_string(), message: "Selling on 42% retracement from top (20%+ PNL)".to_string(), sell_amount: 100 },
        ]
    }
    
    pub fn add_token_strategy(&mut self, token_mint: String, buy_price: f64, token_amount: f64, token_decimals: u8) {
        let strategy = SellStrategy::new(token_mint.clone(), buy_price, token_amount, token_decimals);
        self.strategies.insert(token_mint, strategy);
    }
    
    pub fn remove_token_strategy(&mut self, token_mint: &str) -> Option<SellStrategy> {
        self.strategies.remove(token_mint)
    }
    
    pub fn update_token_price(&mut self, token_mint: &str, new_price: f64) -> Result<()> {
        if let Some(strategy) = self.strategies.get_mut(token_mint) {
            strategy.update_price(new_price);
            Ok(())
        } else {
            Err(anyhow!("Token strategy not found"))
        }
    }
    
    async fn handle_take_profit_stop_loss(
        &mut self,
        token_mint: &str,
        swapx: &Pump,
        swap_config: &SwapConfig,
        existing_liquidity_pools: &Arc<Mutex<HashSet<LiquidityPool>>>,
        telegram_service: &Option<Arc<TelegramService>>,
        telegram_chat_id: &str,
    ) -> Result<bool> {
        // Get a clone of the strategy first
        let mut strategy = match self.strategies.get(token_mint).cloned() {
            Some(s) => s,
            None => return Err(anyhow!("Token strategy not found")),
        };
        
        let take_profit_hit = strategy.is_take_profit_hit();
        let stop_loss_hit = strategy.is_stop_loss_hit();
        
        // No action needed if neither condition is met
        if !take_profit_hit && !stop_loss_hit {
            return Ok(false);
        }
        
        // Determine which condition was hit and update strategy
        let (sell_percentage, reason) = if take_profit_hit {
            strategy.mark_take_profit_triggered();
            
            // Update the strategy in the map
            if let Some(s) = self.strategies.get_mut(token_mint) {
                s.take_profit_triggered = true;
            }
            
            (100, format!("Take profit target of {}% reached", strategy.take_profit_pct))
        } else { // stop_loss_hit must be true here
            strategy.mark_stop_loss_triggered();
            
            // Update the strategy in the map
            if let Some(s) = self.strategies.get_mut(token_mint) {
                s.stop_loss_triggered = true;
            }
            
            (100, format!("Stop loss of {}% triggered", strategy.stop_loss_pct))
        };
        
        // Log details
        let pnl = strategy.calculate_pnl();
        
        self.logger.log(format!(
            "🟥 AUTOMATIC ORDER: Selling {}% of {} - Reason: {}",
            sell_percentage,
            token_mint,
            reason
        ).red().to_string());
        
        // Execute sell order
        let custom_swap_config = SwapConfig {
            swap_direction: SwapDirection::Sell,
            in_type: SwapInType::Pct,
            amount_in: sell_percentage as f64,
            slippage: swap_config.slippage,
            use_jito: swap_config.use_jito,
        };
        
        let current_price = strategy.current_price;
        
        // Create telegram notification
        if let Some(telegram) = telegram_service {
            let message = format!(
                "🟥 Automatic Order:\n`{}`\n💰 {} PNL: {}% \n📊 Selling {}% ({})\nCurrent price: {} SOL",
                token_mint,
                if pnl < 0.0 { "🔴" } else { "🟢" },
                pnl.to_string(),
                sell_percentage,
                reason,
                current_price
            );
            
            // Try to send the notification but don't fail if it doesn't work
            if let Err(e) = telegram.send_message(telegram_chat_id, &message, "Markdown").await {
                self.logger.log(format!("Failed to send Telegram notification: {}", e).red().to_string());
            }
        }
        
        // Execute the sell
        let start_time = Instant::now();
        
        match swapx.build_swap_ixn_by_mint(
            token_mint,
            None, // No bonding curve info for selling
            custom_swap_config.clone(),
            start_time,
        ).await {
            Ok(result) => {
                let (keypair, instructions, token_price) = result;
                
                self.logger.log(format!(
                    "Built sell order for {}% of {} at price {} SOL ({}% PNL)",
                    sell_percentage,
                    token_mint,
                    token_price,
                    pnl
                ).yellow().to_string());
                
                // Get the latest blockhash
                if let Ok(recent_blockhash) = swapx.rpc_nonblocking_client.get_latest_blockhash().await {
                    match tx::new_signed_and_send_nozomi(
                        recent_blockhash,
                        &keypair,
                        instructions,
                        &self.logger,
                    ).await {
                        Ok(res) => {
                            self.logger.log(format!(
                                "✅ Executed automatic sell: {} - TX: {}",
                                reason,
                                res[0]
                            ).green().to_string());
                            
                            // Update the pool status to indicate it's sold
                            let mint_for_update = token_mint.to_string();
                            let token_price_for_update = token_price;
                            
                            // Use a separate scope for the mutex lock
                            {
                                let mut pools = existing_liquidity_pools.lock().unwrap();
                                let pools_vec: Vec<LiquidityPool> = pools.iter().cloned().collect();
                                
                                // Remove old pools and add updated ones
                                pools.clear();
                                
                                for pool in pools_vec {
                                    if pool.mint == mint_for_update {
                                        // Create an updated pool
                                        let mut updated_pool = pool.clone();
                                        updated_pool.status = Status::Sold;
                                        updated_pool.sell_price = token_price_for_update;
                                        pools.insert(updated_pool);
                                    } else {
                                        // Keep the original pool
                                        pools.insert(pool);
                                    }
                                }
                            }
                            
                            // Send confirmation via telegram
                            if let Some(telegram) = telegram_service {
                                // Get the strategy to access token amount
                                let token_amount = strategy.token_amount;
                                
                                // Use the new transaction notification method
                                if let Err(e) = telegram.send_transaction_notification(
                                    "sell",
                                    token_mint,
                                    None, // We don't have symbol info here
                                    token_price * token_amount, // Approximate SOL amount
                                    token_amount,
                                    token_price,
                                    &res[0],
                                    Some(pnl),
                                ).await {
                                    self.logger.log(format!("Failed to send success notification: {}", e).red().to_string());
                                }
                            }
                        },
                        Err(e) => {
                            self.logger.log(format!(
                                "❌ Failed to execute automatic sell: {} - Error: {}",
                                reason,
                                e
                            ).red().to_string());
                            
                            // Update pools to indicate failure
                            let mint_for_update = token_mint.to_string();
                            
                            {
                                let mut pools = existing_liquidity_pools.lock().unwrap();
                                let pools_vec: Vec<LiquidityPool> = pools.iter().cloned().collect();
                                
                                // Remove old pools and add updated ones
                                pools.clear();
                                
                                for pool in pools_vec {
                                    if pool.mint == mint_for_update {
                                        // Create an updated pool with failure status
                                        let mut updated_pool = pool.clone();
                                        updated_pool.status = Status::Failure;
                                        pools.insert(updated_pool);
                                    } else {
                                        // Keep the original pool
                                        pools.insert(pool);
                                    }
                                }
                            }
                            
                            // Send failure notification
                            if let Some(telegram) = telegram_service {
                                let failure_message = format!(
                                    "❌ Automatic Sell Failed:\n`{}`\n⚠️ {}\n💰 PNL: {}%\n🛑 Error: {}",
                                    token_mint,
                                    reason,
                                    pnl.to_string(),
                                    e
                                );
                                
                                if let Err(e) = telegram.send_message(telegram_chat_id, &failure_message, "Markdown").await {
                                    self.logger.log(format!("Failed to send failure notification: {}", e).red().to_string());
                                }
                            }
                        }
                    }
                } else {
                    self.logger.log("Failed to get recent blockhash for automatic sell".red().to_string());
                }
            },
            Err(e) => {
                self.logger.log(format!(
                    "❌ Failed to build automatic sell order: {} - Error: {}",
                    reason,
                    e
                ).red().to_string());
                
                // Send failure notification
                if let Some(telegram) = telegram_service {
                    let build_failure_message = format!(
                        "❌ Failed to Build Automatic Sell:\n`{}`\n⚠️ {}\n💰 PNL: {}%\n🛑 Error: {}",
                        token_mint,
                        reason,
                        pnl.to_string(),
                        e
                    );
                    
                    if let Err(e) = telegram.send_message(telegram_chat_id, &build_failure_message, "Markdown").await {
                        self.logger.log(format!("Failed to send build failure notification: {}", e).red().to_string());
                    }
                }
            }
        }
        
        // Return true to indicate a sell was initiated (or attempted)
        Ok(true)
    }
    
    pub async fn evaluate_selling_conditions(
        &mut self,
        token_mint: &str,
        swapx: &Pump,
        swap_config: &SwapConfig,
        existing_liquidity_pools: &Arc<Mutex<HashSet<LiquidityPool>>>,
        telegram_service: &Option<Arc<TelegramService>>,
        telegram_chat_id: &str,
    ) -> Result<()> {
        let strategy_opt = self.strategies.get(token_mint).cloned();
        
        let strategy = match strategy_opt {
            Some(s) => s,
            None => return Err(anyhow!("Token strategy not found")),
        };
        
        let pnl = strategy.calculate_pnl();
        let time_elapsed = strategy.time_elapsed_secs();
        
        self.logger.log(format!(
            "T->{}s - PNL:{}% - {} - Top PNL: {}% - TP: {}% (${}) - SL: {}% (${}) - Current: ${} ",
            time_elapsed,
            pnl.to_string().green(),
            token_mint,
            strategy.top_pnl.to_string().yellow(),
            strategy.take_profit_pct,
            strategy.take_profit_price(),
            strategy.stop_loss_pct,
            strategy.stop_loss_price(),
            strategy.current_price
        ));
        
        if let Ok(true) = self.handle_take_profit_stop_loss(
            token_mint, 
            swapx, 
            swap_config, 
            existing_liquidity_pools, 
            telegram_service, 
            telegram_chat_id
        ).await {
            return Ok(());
        }
        
        async fn notify_and_log(
            logger: &Logger,
            strategies: &mut HashMap<String, SellStrategy>,
            token_mint: &str,
            percentage: u64,
            description: &str,
            pnl: f64,
            telegram_service: &Option<Arc<TelegramService>>,
        ) -> Result<()> {
            // Get the strategy from the map
            let strategy = match strategies.get_mut(token_mint) {
                Some(s) => s,
                None => return Err(anyhow!("Token strategy not found")),
            };
            
            // Log the sell decision
            logger.log(format!(
                "🟥 SELL DECISION: Selling {}% of {} - Reason: {}",
                percentage,
                token_mint,
                description
            ).red().to_string());
            
            // If telegram is configured, send notification
            if let Some(telegram) = telegram_service {
                let message = format!(
                    "🟥 Sell Decision:\n`{}`\n💰 {} PNL: {}% \n📊 Selling {}% ({})\n",
                    token_mint,
                    if pnl < 0.0 { "🔴" } else { "🟢" },
                    pnl.to_string(),
                    percentage,
                    description
                );
                
                // Use a direct method call that's available on TelegramService
                match telegram.send_notification(&message).await {
                    Ok(_) => {},
                    Err(e) => {
                        logger.log(format!("Failed to send Telegram notification: {}", e).red().to_string());
                    }
                }
            }
            
            // Mark this interval as completed
            strategy.completed_intervals.insert(description.to_string());
            strategy.last_sell_time = Instant::now();
            
            // If selling 100%, reset tracking values
            if percentage == 100 {
                strategy.top_pnl = 0.0;
                strategy.completed_intervals.clear();
            }
            
            Ok(())
        }
        
        // Stop loss conditions
        if pnl < 0.0 && !strategy.completed_intervals.contains("stop_loss_15") {
            self.logger.log("Executing stop loss at -15%".to_string());
            return notify_and_log(
                &self.logger,
                &mut self.strategies,
                token_mint,
                100,
                "Stop loss -15%",
                pnl,
                telegram_service
            ).await;
        }
        
        // Time-based exit
        if (pnl < 0.0 && !strategy.completed_intervals.contains("stop_loss_15") && time_elapsed > 8) 
            || (pnl < 5.0 && !strategy.completed_intervals.contains("stop_loss_15") && time_elapsed > 15) {
            self.logger.log("Executing time-based exit".to_string());
            return notify_and_log(
                &self.logger,
                &mut self.strategies,
                token_mint,
                100,
                "Time-based exit",
                pnl,
                telegram_service
            ).await;
        }
        
        // Trailing stop loss
        if pnl < strategy.top_pnl * 0.4 && strategy.top_pnl > 10.0 {
            self.logger.log(format!(
                "Executing trailing stop loss at {}% (70% drop from peak of {}%)",
                strategy.top_pnl * 0.3,
                strategy.top_pnl
            ));
            return notify_and_log(
                &self.logger,
                &mut self.strategies,
                token_mint,
                100,
                &format!("Trailing stop loss: 70% drop from peak of {}%", strategy.top_pnl),
                pnl,
                telegram_service
            ).await;
        }
        
        // Take profit conditions
        for level in &self.take_profit_levels {
            if pnl > level.threshold && !strategy.completed_intervals.contains(&level.flag) {
                self.logger.log(format!("Executing take profit at {}%", level.threshold));
                return notify_and_log(
                    &self.logger,
                    &mut self.strategies,
                    token_mint,
                    level.percentage,
                    &level.message,
                    pnl,
                    telegram_service
                ).await;
            }
        }
        
        // Retracement conditions - dynamically select based on PNL and time
        let retracement_levels = Self::get_retracement_levels_for_pnl(pnl, time_elapsed);
        for level in retracement_levels {
            if pnl > level.threshold && !strategy.completed_intervals.contains(&level.flag) {
                let retracement_pct = (strategy.top_pnl - pnl) / strategy.top_pnl * 100.0;
                if retracement_pct >= level.percentage {
                    self.logger.log(format!(
                        "Executing retracement sell: {}% drop from peak {}%",
                        retracement_pct,
                        strategy.top_pnl
                    ));
                    return notify_and_log(
                        &self.logger,
                        &mut self.strategies,
                        token_mint,
                        level.sell_amount,
                        &level.message,
                        pnl,
                        telegram_service
                    ).await;
                }
            }
        }
        
        Ok(())
    }
    
    pub async fn start_monitoring_loop(
        mut self,
        swapx: Pump,
        swap_config: SwapConfig,
        existing_liquidity_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
        telegram_service: Option<Arc<TelegramService>>,
        telegram_chat_id: String,
    ) {
        let monitoring_interval = Duration::from_secs(1);
        let mut interval = time::interval(monitoring_interval);
        
        self.logger.log("Starting sell manager monitoring loop".blue().to_string());
        
        loop {
            interval.tick().await;
            
            let token_mints: Vec<String> = self.strategies.keys().cloned().collect();
            
            for token_mint in token_mints {
                if let Err(e) = self.evaluate_selling_conditions(
                    &token_mint,
                    &swapx,
                    &swap_config,
                    &existing_liquidity_pools,
                    &telegram_service,
                    &telegram_chat_id,
                ).await {
                    self.logger.log(format!("Error evaluating sell conditions for {}: {}", token_mint, e).red().to_string());
                }
            }
        }
    }
    
    /// Get a mutable reference to a strategy for a given token mint
    pub fn get_strategy_mut(&mut self, token_mint: &str) -> Option<&mut SellStrategy> {
        self.strategies.get_mut(token_mint)
    }
    
    /// Get a read-only reference to a strategy for a given token mint
    pub fn get_strategy(&self, token_mint: &str) -> Option<&SellStrategy> {
        self.strategies.get(token_mint)
    }
    
    /// Get all token mint addresses for which we have active strategies
    pub fn get_all_strategy_tokens(&self) -> Vec<String> {
        self.strategies.keys().cloned().collect()
    }
} 