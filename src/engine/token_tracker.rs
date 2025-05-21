use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use colored::Colorize;
use tokio::task::JoinHandle;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_client::nonblocking::rpc_client::RpcClient;
use anyhow::{Result, anyhow};
use spl_token::solana_program::native_token::lamports_to_sol;

use crate::common::logger::Logger;
use crate::engine::monitor::TransactionType;

/// Represents detailed information about a token
#[derive(Debug)]
pub struct ExtendedTokenInfo {
    pub token_mint: String,
    pub token_name: Option<String>,
    pub token_symbol: Option<String>,
    pub current_token_price: f64,
    pub max_token_price: f64,
    pub initial_price: f64,
    pub total_supply: u64,
    pub buy_tx_num: u32,
    pub sell_tx_num: u32,
    pub token_mint_timestamp: Instant,
    pub launcher_sol_balance: Option<f64>,
    pub market_cap: Option<f64>,
    pub volume: Option<f64>,
    pub dev_buy_amount: Option<f64>,
    pub bundle_check: Option<bool>,
    pub active_task: Option<JoinHandle<()>>,
    pub is_monitored: bool,
    pub dev_wallet: Option<String>,
    pub token_amount: Option<f64>,
}

impl ExtendedTokenInfo {
    pub fn new(
        token_mint: String, 
        token_name: Option<String>,
        token_symbol: Option<String>,
        current_price: f64,
        total_supply: u64, 
        dev_buy_amount: Option<f64>,
        launcher_sol_balance: Option<f64>,
        bundle_check: Option<bool>,
        dev_wallet: Option<String>,
    ) -> Self {
        Self {
            token_mint,
            token_name,
            token_symbol,
            current_token_price: current_price,
            max_token_price: current_price,
            initial_price: current_price,
            total_supply,
            buy_tx_num: 1, // Start with 1 since we're detecting the initial transaction
            sell_tx_num: 0,
            token_mint_timestamp: Instant::now(),
            launcher_sol_balance,
            market_cap: Some(current_price * (total_supply as f64)),
            volume: dev_buy_amount,
            dev_buy_amount,
            bundle_check,
            active_task: None,
            is_monitored: false,
            dev_wallet,
            token_amount: None,
        }
    }
    
    /// Update token price and related statistics
    pub fn update_price(&mut self, new_price: f64, is_buy: bool) {
        self.current_token_price = new_price;
        
        // Update max price if current price is higher
        if new_price > self.max_token_price {
            self.max_token_price = new_price;
        }
        
        // Update transaction counts
        if is_buy {
            self.buy_tx_num += 1;
        } else {
            self.sell_tx_num += 1;
        }
        
        // Update market cap based on new price
        if let Some(mc) = self.market_cap.as_mut() {
            *mc = new_price * (self.total_supply as f64);
        }
        
        // Update volume metrics
        if let Some(vol) = self.volume.as_mut() {
            *vol += new_price;
        }
    }
    
    /// Calculate price delta percentage from max price
    pub fn price_delta_percent(&self) -> f64 {
        if self.max_token_price == 0.0 {
            return 0.0;
        }
        
        ((self.max_token_price - self.current_token_price) / self.max_token_price) * 100.0
    }
    
    /// Check if token has been active for longer than the specified duration
    pub fn exceeds_time_threshold(&self, threshold: Duration) -> bool {
        self.token_mint_timestamp.elapsed() > threshold
    }
    
    /// Convert to a telegram-compatible TokenInfo
    pub fn to_telegram_token_info(&self) -> crate::services::telegram::TokenInfo {
        // Calculate buy/sell ratio
        let total_tx = self.buy_tx_num + self.sell_tx_num;
        let buy_ratio = if total_tx > 0 {
            Some((self.buy_tx_num as f64 / total_tx as f64 * 100.0) as i32)
        } else {
            None
        };
        
        let sell_ratio = if total_tx > 0 {
            Some((self.sell_tx_num as f64 / total_tx as f64 * 100.0) as i32)
        } else {
            None
        };
        
        // Estimate volume buy/sell based on ratio
        let volume_value = self.volume.unwrap_or(0.0);
        let volume_buy = if let Some(ratio) = buy_ratio {
            Some(volume_value * (ratio as f64 / 100.0))
        } else {
            None
        };
        
        let volume_sell = if let Some(ratio) = sell_ratio {
            Some(volume_value * (ratio as f64 / 100.0))
        } else {
            None
        };
        
        // Token price relative to initial price (simplified as 1.0x for now)
        // In a real implementation, you'd compare to some base price
        let token_price = Some(1.0); // Placeholder, should be calculated from real data
        
        // Get SOL balance from launcher_sol_balance
        let sol_balance = self.launcher_sol_balance;
        
        crate::services::telegram::TokenInfo {
            address: self.token_mint.clone(),
            name: self.token_name.clone(),
            symbol: self.token_symbol.clone(),
            market_cap: self.market_cap,
            volume: self.volume,
            buy_sell_count: Some((self.buy_tx_num + self.sell_tx_num) as i32),
            dev_buy_amount: self.dev_buy_amount,
            launcher_sol_balance: self.launcher_sol_balance,
            bundle_check: self.bundle_check,
            // ATH is equivalent to max_token_price in this context
            ath: Some(self.max_token_price),
            // Add new fields from screenshot
            buy_ratio,
            sell_ratio,
            total_buy_sell: Some(total_tx as i32),
            volume_buy,
            volume_sell,
            token_price,
            dev_wallet: self.dev_wallet.clone(),
            sol_balance,
        }
    }
    
    // Manual implementation of clone since JoinHandle doesn't implement Clone
    pub fn clone_without_task(&self) -> Self {
        Self {
            token_mint: self.token_mint.clone(),
            token_name: self.token_name.clone(),
            token_symbol: self.token_symbol.clone(),
            current_token_price: self.current_token_price,
            max_token_price: self.max_token_price,
            initial_price: self.initial_price,
            total_supply: self.total_supply,
            buy_tx_num: self.buy_tx_num,
            sell_tx_num: self.sell_tx_num,
            token_mint_timestamp: self.token_mint_timestamp,
            launcher_sol_balance: self.launcher_sol_balance,
            market_cap: self.market_cap,
            volume: self.volume,
            dev_buy_amount: self.dev_buy_amount,
            bundle_check: self.bundle_check,
            active_task: None, // Don't clone the task
            is_monitored: self.is_monitored,
            dev_wallet: self.dev_wallet.clone(), // Include the dev wallet in the clone
            token_amount: self.token_amount,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenFilter {
    pub min_launcher_sol: f64,
    pub max_launcher_sol: f64,
    pub min_market_cap: f64,
    pub max_market_cap: f64,
    pub min_volume: f64,
    pub max_volume: f64,
    pub min_buy_sell_count: u32,
    pub max_buy_sell_count: u32,
    pub price_delta_threshold: f64,
    pub time_delta_threshold: u64,
}

impl Default for TokenFilter {
    fn default() -> Self {
        Self {
            min_launcher_sol: 0.1,
            max_launcher_sol: 100.0,
            min_market_cap: 0.1,
            max_market_cap: 1_000_000.0,
            min_volume: 0.1,
            max_volume: 1_000_000.0,
            min_buy_sell_count: 1,
            max_buy_sell_count: 1000,
            price_delta_threshold: 10.0,
            time_delta_threshold: 3600,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackedTokenInfo {
    pub token_mint: String,
    pub token_name: Option<String>,
    pub token_symbol: Option<String>,
    pub price: f64,
    pub market_cap: f64,
    pub volume: f64,
    pub buy_count: u32,
    pub sell_count: u32,
    pub last_updated: Instant,
    pub first_seen: Instant,
    pub price_history: Vec<(Instant, f64)>,
    pub highest_price: f64,
    pub lowest_price: f64,
    pub launcher_sol: f64,
}

impl TrackedTokenInfo {
    pub fn new(token_mint: String, token_name: Option<String>, token_symbol: Option<String>, price: f64, launcher_sol: f64) -> Self {
        let now = Instant::now();
        Self {
            token_mint,
            token_name,
            token_symbol,
            price,
            market_cap: price,
            volume: price,
            buy_count: 1,
            sell_count: 0,
            last_updated: now,
            first_seen: now,
            price_history: vec![(now, price)],
            highest_price: price,
            lowest_price: price,
            launcher_sol,
        }
    }
    
    pub fn update_price(&mut self, new_price: f64) {
        let now = Instant::now();
        self.price = new_price;
        self.last_updated = now;
        self.price_history.push((now, new_price));
        
        if new_price > self.highest_price {
            self.highest_price = new_price;
        }
        
        if new_price < self.lowest_price {
            self.lowest_price = new_price;
        }
        
        // Keep only the last 100 price points
        if self.price_history.len() > 100 {
            self.price_history.remove(0);
        }
    }
    
    pub fn record_buy(&mut self, amount: f64) {
        self.buy_count += 1;
        self.volume += amount;
    }
    
    pub fn record_sell(&mut self, amount: f64) {
        self.sell_count += 1;
        self.volume += amount;
    }
    
    pub fn calculate_price_change(&self, duration: Duration) -> Option<f64> {
        let now = Instant::now();
        let threshold_time = now.checked_sub(duration)?;
        
        // Find the oldest price point within the specified duration
        let oldest_price_in_duration = self.price_history.iter()
            .find(|(time, _)| *time >= threshold_time)
            .map(|(_, price)| *price);
        
        match oldest_price_in_duration {
            Some(old_price) => {
                if old_price == 0.0 {
                    return Some(0.0);
                }
                Some((self.price - old_price) / old_price * 100.0)
            },
            None => None,
        }
    }
    
    pub fn passes_filter(&self, filter: &TokenFilter) -> bool {
        // Check launcher SOL bounds
        if self.launcher_sol < filter.min_launcher_sol || self.launcher_sol > filter.max_launcher_sol {
            return false;
        }
        
        // Check market cap bounds
        if self.market_cap < filter.min_market_cap || self.market_cap > filter.max_market_cap {
            return false;
        }
        
        // Check volume bounds
        if self.volume < filter.min_volume || self.volume > filter.max_volume {
            return false;
        }
        
        // Check transaction count bounds
        let total_transactions = self.buy_count + self.sell_count;
        if total_transactions < filter.min_buy_sell_count || total_transactions > filter.max_buy_sell_count {
            return false;
        }
        
        // Check price delta if we have enough history
        if let Some(price_change) = self.calculate_price_change(Duration::from_secs(filter.time_delta_threshold)) {
            if price_change.abs() < filter.price_delta_threshold {
                return false;
            }
        }
        
        true
    }
}

/// Manages token tracking and trading
pub struct TokenTracker {
    tokens: Arc<RwLock<HashMap<String, TrackedTokenInfo>>>,
    extended_tokens: Arc<Mutex<HashMap<String, ExtendedTokenInfo>>>,
    filter: TokenFilter,
    logger: Logger,
    price_check_threshold: f64,
    time_threshold: Duration,
    min_market_cap: f64,
    max_market_cap: f64,
    min_volume: f64,
    max_volume: f64,
    min_buy_sell_count: u32,
    max_buy_sell_count: u32,
    min_launcher_sol_balance: f64,
    max_launcher_sol_balance: f64,
}

impl TokenTracker {
    pub fn new(
        logger: Logger,
        price_check_threshold: f64,
        time_threshold_secs: u64,
        min_market_cap: f64,
        max_market_cap: f64,
        min_volume: f64,
        max_volume: f64,
        min_buy_sell_count: u32,
        max_buy_sell_count: u32,
        min_launcher_sol_balance: f64,
        max_launcher_sol_balance: f64,
    ) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
            extended_tokens: Arc::new(Mutex::new(HashMap::new())),
            filter: TokenFilter {
                min_launcher_sol: min_launcher_sol_balance,
                max_launcher_sol: max_launcher_sol_balance,
                min_market_cap: min_market_cap,
                max_market_cap: max_market_cap,
                min_volume: min_volume,
                max_volume: max_volume,
                min_buy_sell_count: min_buy_sell_count,
                max_buy_sell_count: max_buy_sell_count,
                price_delta_threshold: 10.0,
                time_delta_threshold: 3600,
            },
            logger,
            price_check_threshold,
            time_threshold: Duration::from_secs(time_threshold_secs),
            min_market_cap,
            max_market_cap,
            min_volume,
            max_volume,
            min_buy_sell_count,
            max_buy_sell_count,
            min_launcher_sol_balance,
            max_launcher_sol_balance,
        }
    }
    
    pub fn start(&self) {
        self.logger.log("Starting token tracker...".green().to_string());
    }
    
    pub async fn add_token(&self, 
        token_mint: String, 
        initial_price: f64, 
        launcher_sol: f64,
        dev_buy_amount: Option<f64>,
        launcher_sol_balance: Option<f64>,
        bundle_check: Option<bool>,
        token_name: Option<String>,
        token_symbol: Option<String>
    ) -> Result<()> {
        // Add to tracked tokens
        let mut tokens = self.tokens.write().unwrap();
        
        if tokens.contains_key(&token_mint) {
            return Err(anyhow!("Token {} already tracked", token_mint));
        }
        
        let tracked_token = TrackedTokenInfo::new(
            token_mint.clone(),
            token_name.clone(),
            token_symbol.clone(),
            initial_price,
            launcher_sol
        );
        
        tokens.insert(token_mint.clone(), tracked_token);
        
        // Also add to extended tokens for additional data
        let mut extended_tokens = self.extended_tokens.lock().unwrap();
        
        let extended_token = ExtendedTokenInfo::new(
            token_mint.clone(), 
            token_name, 
            token_symbol,
            initial_price, 
            10_000_000, // Default total supply
            dev_buy_amount,
            launcher_sol_balance,
            bundle_check,
            None,
        );
        
        extended_tokens.insert(token_mint.clone(), extended_token);
        //initial_price is in lamports
        // Log the token addition
        self.logger.log(format!(
            "Added token to tracker: {} with initial price: {} lamports", 
            token_mint,
            initial_price as u64
        ).green().to_string());
        
        Ok(())
    }
    
    pub async fn update_token_price(&self, token_mint: &str, new_price: f64) -> bool {
        let mut tokens = self.tokens.write().unwrap();
        
        if let Some(token) = tokens.get_mut(token_mint) {
            token.update_price(new_price);
            
            // Update the extended token info as well
            let mut extended_tokens = self.extended_tokens.lock().unwrap();
            if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
                extended_token.current_token_price = new_price;
                
                // Only update max price if new price is higher
                if new_price > extended_token.max_token_price {
                    extended_token.max_token_price = new_price;
                }
                
                // Update market cap based on new price
                extended_token.market_cap = Some(new_price * (extended_token.total_supply as f64));
            }
            
            true
        } else {
            false
        }
    }
    
    pub async fn record_transaction(&self, token_mint: &str, transaction_type: TransactionType, amount: f64) -> bool {
        // First get the token from tokens map
        let token_exists = {
            let tokens = self.tokens.read().unwrap();
            tokens.contains_key(token_mint)
        };
        
        if !token_exists {
            return false;
        }
        
        // Update the token in the tokens map
        {
            let mut tokens = self.tokens.write().unwrap();
            
            if let Some(token) = tokens.get_mut(token_mint) {
                match transaction_type {
                    TransactionType::Buy => {
                        token.record_buy(amount);
                    },
                    TransactionType::Sell => {
                        token.record_sell(amount);
                    },
                    TransactionType::Mint => {
                        // Handle mint case if needed
                    }
                }
            }
        }
        
        // Update the extended token info
        let _token_mint_str = token_mint.to_string();
        
        // Update extended token info in a separate scope
        {
            let mut extended_tokens = self.extended_tokens.lock().unwrap();
            if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
                match transaction_type {
                    TransactionType::Buy => {
                        extended_token.buy_tx_num += 1;
                        extended_token.volume = Some(extended_token.volume.unwrap_or(0.0) + amount);
                        
                        // Update market cap based on current price and total supply
                        if let Some(market_cap) = extended_token.market_cap.as_mut() {
                            *market_cap = extended_token.current_token_price * (extended_token.total_supply as f64);
                        }
                    },
                    TransactionType::Sell => {
                        extended_token.sell_tx_num += 1;
                        extended_token.volume = Some(extended_token.volume.unwrap_or(0.0) + amount);
                        
                        // Update market cap based on current price and total supply
                        if let Some(market_cap) = extended_token.market_cap.as_mut() {
                            *market_cap = extended_token.current_token_price * (extended_token.total_supply as f64);
                        }
                    },
                    TransactionType::Mint => {
                        // Handle mint case if needed
                    }
                }
                
                // Log the updated metrics for debugging
                self.logger.log(format!(
                    "Updated token metrics for {}: Price={:.8}, Volume={:.2}, Buy Count={}, Sell Count={}, Market Cap={:.2}",
                    token_mint,
                    extended_token.current_token_price,
                    extended_token.volume.unwrap_or(0.0),
                    extended_token.buy_tx_num,
                    extended_token.sell_tx_num,
                    extended_token.market_cap.unwrap_or(0.0)
                ).cyan().to_string());
            }
        }
        
        // Check if token now meets criteria after this transaction update
        // We'll do this directly instead of spawning a task to avoid borrowing issues
        let _ = self.update_token_monitored_status(token_mint).await;
        
        true
    }
    
    pub async fn get_token_info(&self, token_mint: &str) -> Option<TrackedTokenInfo> {
        let tokens = self.tokens.read().unwrap();
        tokens.get(token_mint).cloned()
    }
    
    pub async fn get_filtered_tokens(&self) -> Vec<TrackedTokenInfo> {
        let tokens = self.tokens.read().unwrap();
        tokens.values()
            .filter(|token| token.passes_filter(&self.filter))
            .cloned()
            .collect()
    }
    
    pub fn start_token_monitoring(&self, token_mint: String, _callback: impl Fn(TrackedTokenInfo) + Send + 'static) {
        let mut extended_tokens = self.extended_tokens.lock().unwrap();
        
        if let Some(extended_token) = extended_tokens.get_mut(&token_mint) {
            if extended_token.is_monitored {
                self.logger.log(format!("Token {} is already being monitored", token_mint));
                return;
            }
            
            extended_token.is_monitored = true;
            
            self.logger.log(format!("Started monitoring token {}", token_mint).green().to_string());
        } else {
            self.logger.log(format!("Cannot monitor token {} as it doesn't exist", token_mint).red().to_string());
        }
    }
    
    pub fn stop_token_monitoring(&self, token_mint: &str) {
        let mut extended_tokens = self.extended_tokens.lock().unwrap();
        
        if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
            if !extended_token.is_monitored {
                self.logger.log(format!("Token {} is not being monitored", token_mint));
                return;
            }
            
            if let Some(task) = &extended_token.active_task {
                task.abort();
                extended_token.active_task = None;
            }
            
            extended_token.is_monitored = false;
            
            self.logger.log(format!("Stopped monitoring token {}", token_mint).yellow().to_string());
        } else {
            self.logger.log(format!("Cannot stop monitoring token {} as it doesn't exist", token_mint).red().to_string());
        }
    }
    
    /// Clean up tokens based on criteria
    pub async fn clean_expired_tokens(&self) {
        let mut tokens_to_remove = Vec::new();
        
        {
            let tokens = self.tokens.read().unwrap();
            for (mint, token) in tokens.iter() {
                // Check time threshold
                if token.last_updated.elapsed() > self.time_threshold {
                    tokens_to_remove.push(mint.clone());
                    continue;
                }
                
                // Check price delta
                if let Some(change) = token.calculate_price_change(Duration::from_secs(self.filter.time_delta_threshold)) {
                    if change < -self.price_check_threshold {
                        tokens_to_remove.push(mint.clone());
                        continue;
                    }
                }
            }
        }
        
        // Remove tokens and cancel their tasks
        for mint in tokens_to_remove {
            let mut extended_tokens = self.extended_tokens.lock().unwrap();
            if let Some(ext_token) = extended_tokens.get_mut(&mint) {
                if let Some(task) = ext_token.active_task.take() {
                    task.abort();
                }
            }
            
            let mut tokens = self.tokens.write().unwrap();
            if tokens.remove(&mint).is_some() {
                self.logger.log(format!(
                    "[TOKEN REMOVED] => No longer tracking: {}", 
                    mint
                ).yellow().to_string());
            }
        }
    }
    
    /// Get all tracked tokens
    pub fn get_tokens(&self) -> &Arc<RwLock<HashMap<String, TrackedTokenInfo>>> {
        &self.tokens
    }
    
    /// Check if a token is being tracked
    pub async fn is_token_tracked(&self, token_mint: &str) -> bool {
        let tokens = self.tokens.read().unwrap();
        tokens.contains_key(token_mint)
    }
    
    /// Get a specific token
    pub async fn get_token(&self, token_mint: &str) -> Option<TrackedTokenInfo> {
        let tokens = self.tokens.read().unwrap();
        tokens.get(token_mint).cloned()
    }
    
    /// Helper method to safely get token data from an Arc<Mutex<TokenTracker>> without
    /// holding MutexGuard across await points
    pub async fn get_token_safely(tracker: &Arc<Mutex<TokenTracker>>, token_mint: &str) -> Option<TrackedTokenInfo> {
        let tokens_arc = {
            let tracker_guard = tracker.lock().unwrap();
            tracker_guard.tokens.clone()
        };
        
        let tokens = tokens_arc.read().unwrap();
        tokens.get(token_mint).cloned()
    }
    
    /// Safe way to check if token is tracked when using a mutex-wrapped tracker
    pub async fn check_token_tracked_safely(tracker: &Arc<Mutex<TokenTracker>>, token_mint: &str) -> bool {
        let tokens_arc = {
            let tracker_guard = tracker.lock().unwrap();
            tracker_guard.tokens.clone()
        };
        
        let tokens = tokens_arc.read().unwrap();
        tokens.contains_key(token_mint)
    }

    /// Get token lifetime in milliseconds since it was first tracked
    pub fn get_token_lifetime_ms(&self, token_mint: &str) -> Option<u64> {
        let extended_tokens = self.extended_tokens.lock().unwrap();
        if let Some(extended_token) = extended_tokens.get(token_mint) {
            Some(extended_token.token_mint_timestamp.elapsed().as_millis() as u64)
        } else {
            None
        }
    }

    /// Fetch a token launcher's SOL balance
    pub async fn fetch_launcher_sol_balance(
        client: Arc<RpcClient>,
        launcher_pubkey: &Pubkey,
    ) -> Result<f64> {
        match client.get_balance(launcher_pubkey).await {
            Ok(lamports) => Ok(lamports_to_sol(lamports)),
            Err(e) => Err(anyhow!("Failed to get launcher balance: {}", e)),
        }
    }

    /// Check if a token is ready for trading based on criteria
    pub async fn is_token_ready_for_trading(&self, token_mint: &str) -> bool {
        // First get all the data we need from the token
        let (
            token_lifetime_ms,
            buy_tx_num,
            sell_tx_num,
            market_cap,
            volume,
            launcher_sol_balance,
            dev_buy_amount,
            logger
        ) = {
            // Scope for the lock to ensure it's released before any await points
            if let Ok(tokens) = self.extended_tokens.lock() {
                if let Some(token) = tokens.get(token_mint) {
                    let lifetime_ms = token.token_mint_timestamp.elapsed().as_millis() as u64;
                    let buy_count = token.buy_tx_num;
                    let sell_count = token.sell_tx_num;
                    let market_cap_val = token.market_cap;
                    let volume_val = token.volume;
                    let launcher_sol = token.launcher_sol_balance;
                    let dev_buy = token.dev_buy_amount;
                    
                    // Return the values we need
                    (
                        lifetime_ms,
                        buy_count,
                        sell_count,
                        market_cap_val,
                        volume_val,
                        launcher_sol,
                        dev_buy,
                        self.logger.clone()
                    )
                } else {
                    // Token not found
                    self.logger.log(format!(
                        "Token {} not ready for trading: no token information available",
                        token_mint
                    ).yellow().to_string());
                    return false;
                }
            } else {
                // Failed to acquire lock
                self.logger.log(format!(
                    "Token {} not ready for trading: failed to acquire lock",
                    token_mint
                ).yellow().to_string());
                return false;
            }
        };
        
        // Now we can check all criteria without holding any locks
        
        // Get min_last_time from environment
        let min_last_time = std::env::var("MIN_LAST_TIME")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(200000); // Default to 200 seconds (200,000 ms)
        
        // Check token lifetime
        if token_lifetime_ms < min_last_time {
            logger.log(format!(
                "Token {} not ready for trading: lifetime {} ms < required {} ms",
                token_mint, token_lifetime_ms, min_last_time
            ).yellow().to_string());
            return false;
        }
        
        // Check buy/sell count
        let min_buy_sell_count = std::env::var("MIN_NUMBER_OF_BUY_SELL")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(50);
        
        let max_buy_sell_count = std::env::var("MAX_NUMBER_OF_BUY_SELL")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(2000);
        
        let total_tx_count = buy_tx_num + sell_tx_num;
        
        if total_tx_count < min_buy_sell_count || total_tx_count > max_buy_sell_count {
            logger.log(format!(
                "Token {} not ready for trading: transaction count {} outside range {}-{}",
                token_mint, total_tx_count, min_buy_sell_count, max_buy_sell_count
            ).yellow().to_string());
            return false;
        }
        
        // Check market cap
        let min_market_cap = std::env::var("MIN_MARKET_CAP")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(8.0); // Default to 8K
        
        let max_market_cap = std::env::var("MAX_MARKET_CAP")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(15.0); // Default to 15K
        
        if let Some(market_cap_val) = market_cap {
            // Convert to K (thousands)
            let market_cap_k = market_cap_val / 1000.0;
            if market_cap_k < min_market_cap || market_cap_k > max_market_cap {
                logger.log(format!(
                    "Token {} not ready for trading: market cap {:.2}K outside range {:.2}K-{:.2}K",
                    token_mint, market_cap_k, min_market_cap, max_market_cap
                ).yellow().to_string());
                return false;
            }
        } else {
            logger.log(format!(
                "Token {} not ready for trading: no market cap information available",
                token_mint
            ).yellow().to_string());
            return false;
        }
        
        // Check volume
        let min_volume = std::env::var("MIN_VOLUME")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0); // Default to 5K
        
        let max_volume = std::env::var("MAX_VOLUME")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(12.0); // Default to 12K
        
        if let Some(volume_val) = volume {
            // Convert to K (thousands)
            let volume_k = volume_val / 1000.0;
            if volume_k < min_volume || volume_k > max_volume {
                logger.log(format!(
                    "Token {} not ready for trading: volume {:.2}K outside range {:.2}K-{:.2}K",
                    token_mint, volume_k, min_volume, max_volume
                ).yellow().to_string());
                return false;
            }
        } else {
            logger.log(format!(
                "Token {} not ready for trading: no volume information available",
                token_mint
            ).yellow().to_string());
            return false;
        }
        
        // Check launcher SOL balance
        let min_launcher_sol = std::env::var("MIN_LAUNCHER_SOL_BALANCE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0); // Default to 0 SOL
        
        let max_launcher_sol = std::env::var("MAX_LAUNCHER_SOL_BALANCE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0); // Default to 1 SOL
        
        if let Some(launcher_sol) = launcher_sol_balance {
            if launcher_sol < min_launcher_sol || launcher_sol > max_launcher_sol {
                logger.log(format!(
                    "Token {} not ready for trading: launcher SOL balance {} outside range {}-{}",
                    token_mint, launcher_sol, min_launcher_sol, max_launcher_sol
                ).yellow().to_string());
                return false;
            }
        }
        
        // Check dev buy amount
        if let Some(dev_buy) = dev_buy_amount {
            // Get min_dev_buy and max_dev_buy from environment
            let min_dev_buy = std::env::var("MIN_DEV_BUY")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(5.0);
            
            let max_dev_buy = std::env::var("MAX_DEV_BUY")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(30.0);
            
            // Check if dev buy amount is within the acceptable range
            if dev_buy < min_dev_buy || dev_buy > max_dev_buy {
                logger.log(format!(
                    "Token {} not ready for trading: dev buy amount {} SOL outside range {}-{} SOL",
                    token_mint, dev_buy, min_dev_buy, max_dev_buy
                ).yellow().to_string());
                return false;
            }
        }
        
        // If we get here, the token meets all the criteria
        logger.log(format!(
            "Token {} is ready for trading: lifetime {} ms, transaction count {}, buy count {}, sell count {}, market cap {:.2}K, volume {:.2}K",
            token_mint, 
            token_lifetime_ms, 
            total_tx_count, 
            buy_tx_num, 
            sell_tx_num,
            market_cap.unwrap_or(0.0) / 1000.0,
            volume.unwrap_or(0.0) / 1000.0
        ).green().to_string());
        
        true
    }
    
    /// Update a token's is_monitored flag based on its current state
    pub async fn update_token_monitored_status(&self, token_mint: &str) -> bool {
        // First check if token is ready for trading
        let is_ready = self.is_token_ready_for_trading(token_mint).await;
        
        // Update is_monitored flag if criteria are met
        if is_ready {
            let mut extended_tokens = self.extended_tokens.lock().unwrap();
            if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
                let was_monitored = extended_token.is_monitored;
                extended_token.is_monitored = true;
                
                // Return true if token just became monitored (state change)
                return !was_monitored;
            }
        }
        
        false
    }

    /// Update token with transaction info
    pub async fn update_token(&self, token_mint: &str, new_price: f64, is_buy: bool) -> Result<()> {
        // Update the price
        let price_updated = self.update_token_price(token_mint, new_price).await;
        
        // Record the transaction
        let amount = new_price; // Use the price as an approximation of amount if not available
        let updated = self.record_transaction(
            token_mint, 
            if is_buy { TransactionType::Buy } else { TransactionType::Sell },
            amount
        ).await;
        
        if !price_updated && !updated {
            return Err(anyhow!("Token not found: {}", token_mint));
        }
        
        // Update token amount in extended info
        {
            let mut extended_tokens = self.extended_tokens.lock().unwrap();
            if let Some(token) = extended_tokens.get_mut(token_mint) {
                // If it's a buy, add to token amount, if it's a sell, subtract
                let current_amount = token.token_amount.unwrap_or(0.0);
                let new_amount = if is_buy {
                    current_amount + amount // Simplified - in reality would be based on actual token amount
                } else {
                    (current_amount - amount).max(0.0) // Ensure we don't go negative
                };
                token.token_amount = Some(new_amount);
                
                // Update market cap based on current price and total supply
                token.market_cap = Some(new_price * (token.total_supply as f64));
                
                // Update volume
                token.volume = Some(token.volume.unwrap_or(0.0) + amount);
                
                // Log the updated metrics for debugging
                self.logger.log(format!(
                    "Updated token metrics for {}: Price={:.8}, Volume={:.2}, Buy Count={}, Sell Count={}, Market Cap={:.2}",
                    token_mint,
                    token.current_token_price,
                    token.volume.unwrap_or(0.0),
                    token.buy_tx_num,
                    token.sell_tx_num,
                    token.market_cap.unwrap_or(0.0)
                ).cyan().to_string());
            }
        }
        
        // Check if token now meets criteria and update monitored status
        let status_changed = self.update_token_monitored_status(token_mint).await;
        
        if status_changed {
            self.logger.log(format!(
                "🔔 Token {} now meets all criteria after update - Ready for trading!",
                token_mint
            ).green().bold().to_string());
        }
        
        Ok(())
    }

    /// Get a cloned copy of the tokens map Arc
    pub fn get_tokens_map_arc(&self) -> Arc<RwLock<HashMap<String, TrackedTokenInfo>>> {
        self.tokens.clone()
    }

    /// Get a cloned copy of the extended tokens map Arc
    pub fn get_extended_tokens_map_arc(&self) -> Arc<Mutex<HashMap<String, ExtendedTokenInfo>>> {
        self.extended_tokens.clone()
    }

    /// Static version of update_token that can be called on a reference to Arc<Mutex<TokenTracker>>
    pub async fn update_token_static(tracker: &Arc<Mutex<TokenTracker>>, token_mint: &str, new_price: f64, is_buy: bool) -> Result<()> {
        let tokens_arc = {
            let tracker_guard = tracker.lock().unwrap();
            tracker_guard.tokens.clone()
        };
        
        let was_updated = {
            let mut tokens = tokens_arc.write().unwrap();
            match tokens.get_mut(token_mint) {
                Some(token) => {
                    token.update_price(new_price);
                    
                    if is_buy {
                        token.record_buy(1.0); // Assuming amount = 1.0 for simplicity
                    } else {
                        token.record_sell(1.0);
                    }
                    
                    true
                },
                None => false,
            }
        };
        
        if !was_updated {
            return Err(anyhow!("Failed to update token: Token not found"));
        }
        
        // Update token amount in extended info and get logger
        let (logger_clone, token_updated) = {
            let tracker_guard = tracker.lock().unwrap();
            let extended_tokens_map = tracker_guard.get_extended_tokens_map_arc().clone();
            let mut extended_tokens = extended_tokens_map.lock().unwrap();
            
            let logger_clone = tracker_guard.logger.clone();
            let mut token_updated = false;
            
            if let Some(token) = extended_tokens.get_mut(token_mint) {
                // Update token amount based on transaction type
                let amount = new_price; // Use price as an approximation
                let current_amount = token.token_amount.unwrap_or(0.0);
                let new_amount = if is_buy {
                    current_amount + amount
                } else {
                    (current_amount - amount).max(0.0) // Ensure we don't go negative
                };
                token.token_amount = Some(new_amount);
                
                // Also update the price
                token.update_price(new_price, is_buy);
                
                // Update market cap based on current price and total supply
                token.market_cap = Some(new_price * (token.total_supply as f64));
                
                // Update volume
                token.volume = Some(token.volume.unwrap_or(0.0) + amount);
                
                // Log the updated metrics for debugging
                logger_clone.log(format!(
                    "Updated token metrics for {}: Price={:.8}, Volume={:.2}, Buy Count={}, Sell Count={}, Market Cap={:.2}",
                    token_mint,
                    token.current_token_price,
                    token.volume.unwrap_or(0.0),
                    token.buy_tx_num,
                    token.sell_tx_num,
                    token.market_cap.unwrap_or(0.0)
                ).cyan().to_string());
                
                token_updated = true;
            }
            
            (logger_clone, token_updated)
        };
        
        if !token_updated {
            return Err(anyhow!("Failed to update token extended info"));
        }
        
        // Check if the token should be monitored now - but do it without holding locks across await
        let _token_mint_str = token_mint.to_string();
        let tracker_clone = Arc::clone(tracker);
        
        // Helper function to check token status without holding locks across await
        async fn check_token_status(tracker: Arc<Mutex<TokenTracker>>, token_mint: String, logger: Logger) {
            // First, extract all the data we need from the token
            let (
                token_lifetime_ms,
                buy_tx_num,
                sell_tx_num,
                market_cap,
                volume,
                launcher_sol_balance,
                dev_buy_amount,
                extended_tokens_arc
            ) = {
                // Scope for the lock to ensure it's released before any await points
                if let Ok(tracker_guard) = tracker.lock() {
                    // Get a reference to the extended_tokens arc
                    let extended_tokens_arc = tracker_guard.extended_tokens.clone();
                    
                    // Get token data
                    let token_data = {
                        if let Ok(tokens) = extended_tokens_arc.lock() {
                            if let Some(token) = tokens.get(&token_mint) {
                                let lifetime_ms = token.token_mint_timestamp.elapsed().as_millis() as u64;
                                let buy_count = token.buy_tx_num;
                                let sell_count = token.sell_tx_num;
                                let market_cap_val = token.market_cap;
                                let volume_val = token.volume;
                                let launcher_sol = token.launcher_sol_balance;
                                let dev_buy = token.dev_buy_amount;
                                
                                // Return the values we need
                                Some((
                                    lifetime_ms,
                                    buy_count,
                                    sell_count,
                                    market_cap_val,
                                    volume_val,
                                    launcher_sol,
                                    dev_buy
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    
                    if let Some(data) = token_data {
                        (data.0, data.1, data.2, data.3, data.4, data.5, data.6, extended_tokens_arc)
                    } else {
                        // Token not found or couldn't acquire lock
                        logger.log(format!(
                            "Token {} not ready for trading: couldn't get token data",
                            token_mint
                        ).yellow().to_string());
                        return;
                    }
                } else {
                    // Failed to acquire lock
                    logger.log(format!(
                        "Token {} not ready for trading: failed to acquire lock",
                        token_mint
                    ).yellow().to_string());
                    return;
                }
            };
            
            // Now we can check all criteria without holding any locks
            
            // Get min_last_time from environment
            let min_last_time = std::env::var("MIN_LAST_TIME")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(200000); // Default to 200 seconds (200,000 ms)
            
            // Check token lifetime
            if token_lifetime_ms < min_last_time {
                logger.log(format!(
                    "Token {} not ready for trading: lifetime {} ms < required {} ms",
                    token_mint, token_lifetime_ms, min_last_time
                ).yellow().to_string());
                return;
            }
            
            // Check buy/sell count
            let min_buy_sell_count = std::env::var("MIN_NUMBER_OF_BUY_SELL")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(50);
            
            let max_buy_sell_count = std::env::var("MAX_NUMBER_OF_BUY_SELL")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(2000);
            
            let total_tx_count = buy_tx_num + sell_tx_num;
            
            if total_tx_count < min_buy_sell_count || total_tx_count > max_buy_sell_count {
                logger.log(format!(
                    "Token {} not ready for trading: transaction count {} outside range {}-{}",
                    token_mint, total_tx_count, min_buy_sell_count, max_buy_sell_count
                ).yellow().to_string());
                return;
            }
            
            // Check market cap
            let min_market_cap = std::env::var("MIN_MARKET_CAP")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(8.0); // Default to 8K
            
            let max_market_cap = std::env::var("MAX_MARKET_CAP")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(15.0); // Default to 15K
            
            if let Some(market_cap_val) = market_cap {
                // Convert to K (thousands)
                let market_cap_k = market_cap_val / 1000.0;
                if market_cap_k < min_market_cap || market_cap_k > max_market_cap {
                    logger.log(format!(
                        "Token {} not ready for trading: market cap {:.2}K outside range {:.2}K-{:.2}K",
                        token_mint, market_cap_k, min_market_cap, max_market_cap
                    ).yellow().to_string());
                    return;
                }
            } else {
                logger.log(format!(
                    "Token {} not ready for trading: no market cap information available",
                    token_mint
                ).yellow().to_string());
                return;
            }
            
            // Check volume
            let min_volume = std::env::var("MIN_VOLUME")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(5.0); // Default to 5K
            
            let max_volume = std::env::var("MAX_VOLUME")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(12.0); // Default to 12K
            
            if let Some(volume_val) = volume {
                // Convert to K (thousands)
                let volume_k = volume_val / 1000.0;
                if volume_k < min_volume || volume_k > max_volume {
                    logger.log(format!(
                        "Token {} not ready for trading: volume {:.2}K outside range {:.2}K-{:.2}K",
                        token_mint, volume_k, min_volume, max_volume
                    ).yellow().to_string());
                    return;
                }
            } else {
                logger.log(format!(
                    "Token {} not ready for trading: no volume information available",
                    token_mint
                ).yellow().to_string());
                return;
            }
            
            // Check launcher SOL balance
            let min_launcher_sol = std::env::var("MIN_LAUNCHER_SOL_BALANCE")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0); // Default to 0 SOL
            
            let max_launcher_sol = std::env::var("MAX_LAUNCHER_SOL_BALANCE")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0); // Default to 1 SOL
            
            if let Some(launcher_sol) = launcher_sol_balance {
                if launcher_sol < min_launcher_sol || launcher_sol > max_launcher_sol {
                    logger.log(format!(
                        "Token {} not ready for trading: launcher SOL balance {} outside range {}-{}",
                        token_mint, launcher_sol, min_launcher_sol, max_launcher_sol
                    ).yellow().to_string());
                    return;
                }
            }
            
            // Check dev buy amount
            if let Some(dev_buy) = dev_buy_amount {
                // Get min_dev_buy and max_dev_buy from environment
                let min_dev_buy = std::env::var("MIN_DEV_BUY")
                    .ok()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(5.0);
                
                let max_dev_buy = std::env::var("MAX_DEV_BUY")
                    .ok()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(30.0);
                
                // Check if dev buy amount is within the acceptable range
                if dev_buy < min_dev_buy || dev_buy > max_dev_buy {
                    logger.log(format!(
                        "Token {} not ready for trading: dev buy amount {} SOL outside range {}-{} SOL",
                        token_mint, dev_buy, min_dev_buy, max_dev_buy
                    ).yellow().to_string());
                    return;
                }
            }
            
            // If we get here, the token meets all the criteria
            logger.log(format!(
                "Token {} is ready for trading: lifetime {} ms, transaction count {}, buy count {}, sell count {}, market cap {:.2}K, volume {:.2}K",
                token_mint, 
                token_lifetime_ms, 
                total_tx_count, 
                buy_tx_num, 
                sell_tx_num,
                market_cap.unwrap_or(0.0) / 1000.0,
                volume.unwrap_or(0.0) / 1000.0
            ).green().to_string());
            
            // Now update the is_monitored flag
            if let Ok(mut extended_tokens) = extended_tokens_arc.lock() {
                if let Some(extended_token) = extended_tokens.get_mut(&token_mint) {
                    let was_monitored = extended_token.is_monitored;
                    extended_token.is_monitored = true;
                    
                    // Log if token status changed
                    if !was_monitored {
                        logger.log(format!(
                            "🔔 Token {} now meets all criteria after update - Ready for trading!",
                            token_mint
                        ).green().bold().to_string());
                    }
                }
            };
        }
        
        // Spawn the helper function in a new task
        tokio::spawn(check_token_status(tracker_clone, _token_mint_str, logger_clone));
        
        Ok(())
    }

    /// Get token dev buy amount
    pub fn get_token_dev_buy_amount(&self, token_mint: &str) -> Option<f64> {
        let extended_tokens = self.extended_tokens.lock().unwrap();
        extended_tokens.get(token_mint)
            .and_then(|token| token.dev_buy_amount)
    }
    
    /// Get token bundle check status
    pub fn get_token_bundle_check(&self, token_mint: &str) -> Option<bool> {
        let extended_tokens = self.extended_tokens.lock().unwrap();
        extended_tokens.get(token_mint)
            .and_then(|token| token.bundle_check)
    }
    
    /// Check if a token is being monitored
    pub fn is_token_monitored(&self, token_mint: &str) -> bool {
        let extended_tokens = self.extended_tokens.lock().unwrap();
        extended_tokens.get(token_mint)
            .map(|token| token.is_monitored)
            .unwrap_or(false)
    }

    pub async fn get_token_symbol(&self, token_mint: &str) -> Option<String> {
        let extended_tokens = self.extended_tokens.lock().unwrap();
        if let Some(token) = extended_tokens.get(token_mint) {
            token.token_symbol.clone()
        } else {
            None
        }
    }
    
    pub async fn get_token_pnl(&self, token_mint: &str, current_price: f64) -> Option<f64> {
        let extended_tokens = self.extended_tokens.lock().unwrap();
        if let Some(token) = extended_tokens.get(token_mint) {
            // Use the initial_price as the reference price to calculate PNL
            let initial_price = token.initial_price;
            if initial_price > 0.0 {
                let pnl = ((current_price - initial_price) / initial_price) * 100.0;
                return Some(pnl);
            }
        }
        None
    }
}

#[allow(dead_code)]
pub struct TokenStream {
    token_tracker: Arc<RwLock<TokenTracker>>,
    logger: Logger,
    min_dev_buy: f64,
    max_dev_buy: f64,
    min_launcher_sol_balance: f64,
    max_launcher_sol_balance: f64,
    min_market_cap: f64,
    max_market_cap: f64,
    min_volume: f64,
    max_volume: f64,
    min_buy_sell_count: u32,
    max_buy_sell_count: u32,
    price_delta_threshold: f64,
    time_delta_threshold: u64,
    use_bundle: bool,
}

impl TokenStream {
    pub fn new(
        token_tracker: Arc<RwLock<TokenTracker>>,
        logger: Logger,
        min_dev_buy: f64,
        max_dev_buy: f64,
        min_launcher_sol_balance: f64,
        max_launcher_sol_balance: f64,
        min_market_cap: f64,
        max_market_cap: f64,
        min_volume: f64,
        max_volume: f64,
        min_buy_sell_count: u32,
        max_buy_sell_count: u32,
        price_delta_threshold: f64,
        time_delta_threshold: u64,
        use_bundle: bool,
    ) -> Self {
        Self {
            token_tracker,
            logger,
            min_dev_buy,
            max_dev_buy,
            min_launcher_sol_balance,
            max_launcher_sol_balance,
            min_market_cap,
            max_market_cap,
            min_volume,
            max_volume,
            min_buy_sell_count,
            max_buy_sell_count,
            price_delta_threshold,
            time_delta_threshold,
            use_bundle,
        }
    }
    
    pub async fn process_stream_next(&self, stream_event: &StreamEvent) -> Result<()> {
        match stream_event {
            StreamEvent::TokenMint { 
                token_mint, 
                dev_buy_amount, 
                launcher_sol_balance, 
                token_price,
                bundle_check
            } => {
                // Check if dev buy amount is within range
                if *dev_buy_amount < self.min_dev_buy {
                    self.logger.log(format!(
                        "\n\t * [DEV BOUGHT LESS THAN] => {} > {}",
                        self.min_dev_buy, dev_buy_amount
                    ).yellow().to_string());
                    return Ok(());
                } else if *dev_buy_amount > self.max_dev_buy {
                    self.logger.log(format!(
                        "\n\t * [DEV BOUGHT MORE THAN] => {} < {}",
                        self.max_dev_buy, dev_buy_amount
                    ).yellow().to_string());
                    return Ok(());
                }
                
                // Check if launcher SOL balance is within range
                if *launcher_sol_balance < self.min_launcher_sol_balance || 
                   *launcher_sol_balance > self.max_launcher_sol_balance {
                    self.logger.log(format!(
                        "\n\t * [LAUNCHER SOL BALANCE OUT OF RANGE] => {} not in [{}, {}]",
                        launcher_sol_balance, self.min_launcher_sol_balance, self.max_launcher_sol_balance
                    ).yellow().to_string());
                    
                    // Skip if bundle check is not used
                    if !self.use_bundle {
                        return Ok(());
                    }
                }
                
                // Attempt to identify the dev wallet (signer of the mint transaction)
                // In the real implementation, this would query transaction info to find the signer
                // For now, we'll use a placeholder and assume we get this from transaction data
                // This would likely come from analyzing the transaction that created the token
                let dev_wallet = self.identify_dev_wallet(token_mint).await;
                
                if let Some(wallet) = &dev_wallet {
                    self.logger.log(format!(
                        "\n\t * [DEV WALLET IDENTIFIED] => {}",
                        wallet
                    ).green().to_string());
                } else {
                    self.logger.log(format!(
                        "\n\t * [DEV WALLET NOT IDENTIFIED] for token {}",
                        token_mint
                    ).yellow().to_string());
                }
                
                // Create new token
                let token = TrackedTokenInfo::new(
                    token_mint.clone(), 
                    None, // No token name available in this context
                    None, // No token symbol available in this context
                    *token_price, 
                    *launcher_sol_balance
                );
                
                // Track the token - first get a write lock on the tracker
                let tracker = self.token_tracker.clone();
                
                // Add to token maps - use separate scopes for each operation to avoid deadlocks
                {
                    let tracker_guard = tracker.write().unwrap();
                    let tokens_map = tracker_guard.get_tokens_map_arc().clone();
                    let mut tokens_map_guard = tokens_map.write().unwrap();
                    tokens_map_guard.insert(token_mint.clone(), token);
                }
                
                // Add extended info - with proper scoping to avoid deadlocks
                {
                    let tracker_guard = tracker.read().unwrap();
                    let extended_tokens_map = tracker_guard.get_extended_tokens_map_arc().clone();
                    let mut extended_tokens = extended_tokens_map.lock().unwrap();
                    extended_tokens.insert(token_mint.clone(), ExtendedTokenInfo {
                        token_mint: token_mint.clone(),
                        token_name: None, // No token name available in this context
                        token_symbol: None, // No token symbol available in this context
                        current_token_price: *token_price,
                        max_token_price: *token_price,
                        initial_price: *token_price,
                        total_supply: 0,
                        buy_tx_num: 1,
                        sell_tx_num: 0,
                        token_mint_timestamp: Instant::now(),
                        launcher_sol_balance: Some(*launcher_sol_balance),
                        market_cap: Some(*token_price * 0.0), // Will update later
                        volume: Some(*dev_buy_amount),
                        dev_buy_amount: Some(*dev_buy_amount),
                        bundle_check: Some(*bundle_check),
                        active_task: None,
                        is_monitored: false, // Start as false, will be set to true when it meets buy/sell count criteria
                        dev_wallet, // Store the identified dev wallet
                        token_amount: None,
                    });
                }
                
                self.logger.log(format!(
                    "Added new token {} with price {}, dev buy {}, launcher sol {}",
                    token_mint, *token_price, *dev_buy_amount, *launcher_sol_balance
                ).green().to_string());
                
                // Log the buy/sell requirements for this token
                let buy_sell_count_enabled = std::env::var("BUY_SELL_COUNT_ENABLED")
                    .unwrap_or_else(|_| "false".to_string())
                    .to_lowercase() == "true";
                    
                self.logger.log(format!(
                    "Token {} needs to reach buy/sell count between {} and {} (currently: 1) - Filter enabled: {}",
                    token_mint, self.min_buy_sell_count, self.max_buy_sell_count, buy_sell_count_enabled
                ).cyan().to_string());
                
                // We're just adding the token to the tracker, not processing buy action yet
                // because buy/sell count is only 1
                if buy_sell_count_enabled {
                    self.logger.log(format!(
                        "Token {} added to cache. Will monitor until buy/sell count reaches {}",
                        token_mint, self.min_buy_sell_count
                    ).yellow().to_string());
                    
                    // We don't trigger buy action or telegram notification yet
                    return Ok(());
                }
                
                // If buy/sell count filter is not enabled, check other filters and potentially launch buy task
                if self.passes_additional_filters(token_mint).await {
                    // Mark the token as being monitored since it passed all filters
                    {
                        let tracker_guard = tracker.read().unwrap();
                        let extended_tokens_map = tracker_guard.get_extended_tokens_map_arc().clone();
                        let mut extended_tokens = extended_tokens_map.lock().unwrap();
                        if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
                            extended_token.is_monitored = true;
                        }
                    }
                    
                    self.logger.log(format!(
                        "Token {} passed all initial filters - would trigger buy action",
                        token_mint
                    ).green().to_string());
                    
                    // This is where we'd spawn the task using tokio::spawn
                    // and set up the telegram notification
                }
            },
            StreamEvent::TokenTransaction {
                token_mint,
                is_buy,
                amount,
                price,
            } => {
                // Get tracker & token
                let tracker = self.token_tracker.clone();
                let token_exists = {
                    let tracker_guard = tracker.read().unwrap();
                    let tokens_map = tracker_guard.get_tokens_map_arc().clone();
                    let tokens_ref = tokens_map.read().unwrap();
                    tokens_ref.contains_key(token_mint)
                };
                
                if !token_exists {
                    return Ok(());
                }
                
                // Update token info - use separate scopes to avoid deadlocks
                {
                    let tracker_guard = tracker.read().unwrap();
                    let tokens_map = tracker_guard.get_tokens_map_arc().clone();
                    let mut tokens_guard = tokens_map.write().unwrap();
                    
                    if let Some(token) = tokens_guard.get_mut(token_mint) {
                        if *is_buy {
                            token.record_buy(*amount);
                        } else {
                            token.record_sell(*amount);
                        }
                        token.update_price(*price);
                        
                        // Store token data for use outside this scope
                        let token_data = token.clone();
                        let total_count = token.buy_count + token.sell_count;
                        
                        // Drop the tokens_guard before getting the extended tokens
                        drop(tokens_guard);
                        
                        // Update extended info in a separate scope
                        let extended_tokens_map = tracker_guard.get_extended_tokens_map_arc().clone();
                        let mut extended_tokens = extended_tokens_map.lock().unwrap();
                        if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
                            extended_token.update_price(*price, *is_buy);
                            
                            // Check if the token now meets buy/sell count requirements
                            // Check if the BUY_SELL_COUNT_ENABLED environment variable is set
                            let buy_sell_count_enabled = std::env::var("BUY_SELL_COUNT_ENABLED")
                                .unwrap_or_else(|_| "false".to_string())
                                .to_lowercase() == "true";
                            
                            let meets_buy_sell_count = total_count >= self.min_buy_sell_count && 
                                                      total_count <= self.max_buy_sell_count;
                            
                            // Log when token reaches the buy/sell count threshold
                            if buy_sell_count_enabled && meets_buy_sell_count && !extended_token.is_monitored {
                                self.logger.log(format!(
                                    "🔔 Token {} has reached the required buy/sell count: {} (min: {}, max: {})",
                                    token_mint, total_count, self.min_buy_sell_count, self.max_buy_sell_count
                                ).green().bold().to_string());
                                
                                // Make copies of values we need outside of locks
                                let token_market_cap = token_data.market_cap;
                                let token_volume = token_data.volume;
                                let token_launcher_sol = token_data.launcher_sol;
                                let dev_buy_amount = extended_token.dev_buy_amount.unwrap_or(0.0);
                                
                                // Drop the lock before checking additional criteria
                                drop(extended_tokens);
                                
                                // Now check if all other criteria are satisfied
                                let market_cap_enabled = std::env::var("MARKET_CAP_ENABLED")
                                    .unwrap_or_else(|_| "false".to_string())
                                    .to_lowercase() == "true";
                                
                                let volume_enabled = std::env::var("VOLUME_ENABLED")
                                    .unwrap_or_else(|_| "false".to_string())
                                    .to_lowercase() == "true";
                                
                                let launcher_sol_enabled = std::env::var("LAUNCHER_SOL_ENABLED")
                                    .unwrap_or_else(|_| "false".to_string())
                                    .to_lowercase() == "true";
                                
                                let dev_buy_enabled = std::env::var("DEV_BUY_ENABLED")
                                    .unwrap_or_else(|_| "false".to_string())
                                    .to_lowercase() == "true";
                                
                                let meets_market_cap = !market_cap_enabled || 
                                    (token_market_cap >= self.min_market_cap && token_market_cap <= self.max_market_cap);
                                
                                let meets_volume = !volume_enabled || 
                                    (token_volume >= self.min_volume && 
                                     token_volume <= self.max_volume);
                                
                                let meets_launcher_sol = !launcher_sol_enabled || 
                                    (token_launcher_sol >= self.min_launcher_sol_balance && 
                                     token_launcher_sol <= self.max_launcher_sol_balance);
                                
                                let meets_dev_buy = !dev_buy_enabled || 
                                    (dev_buy_amount >= self.min_dev_buy && 
                                     dev_buy_amount <= self.max_dev_buy);
                                
                                // Check if all criteria are met
                                if meets_market_cap && meets_volume && meets_launcher_sol && meets_dev_buy {
                                    self.logger.log(format!(
                                        "✅ Token {} meets ALL criteria - Ready for buy action!",
                                        token_mint
                                    ).green().bold().to_string());
                                    
                                    // Mark token as being monitored - ready for buy action
                                    // Reacquire the lock to update the is_monitored flag
                                    let mut extended_tokens = extended_tokens_map.lock().unwrap();
                                    if let Some(extended_token) = extended_tokens.get_mut(token_mint) {
                                        extended_token.is_monitored = true;
                                    }
                                    
                                    // Token is eligible for telegram notification and buy action
                                    // Note: The actual buy task will be spawned in the enhanced_monitor.rs
                                } else {
                                    self.logger.log(format!(
                                        "❌ Token {} meets buy/sell count but fails other criteria - No buy action",
                                        token_mint
                                    ).yellow().to_string());
                                    
                                    if !meets_market_cap {
                                        self.logger.log(format!(
                                            "  - Market Cap filter failed: {} not in [{}, {}]",
                                            token_market_cap, self.min_market_cap, self.max_market_cap
                                        ).yellow().to_string());
                                    }
                                    
                                    if !meets_volume {
                                        self.logger.log(format!(
                                            "  - Volume filter failed: {} not in [{}, {}]",
                                            token_volume, self.min_volume, self.max_volume
                                        ).yellow().to_string());
                                    }
                                    
                                    if !meets_launcher_sol {
                                        self.logger.log(format!(
                                            "  - Launcher SOL filter failed: {} not in [{}, {}]",
                                            token_launcher_sol, self.min_launcher_sol_balance, self.max_launcher_sol_balance
                                        ).yellow().to_string());
                                    }
                                    
                                    if !meets_dev_buy {
                                        self.logger.log(format!(
                                            "  - Dev Buy filter failed: {} not in [{}, {}]",
                                            dev_buy_amount, self.min_dev_buy, self.max_dev_buy
                                        ).yellow().to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn passes_additional_filters(&self, token_mint: &str) -> bool {
        let token_tracker = self.token_tracker.clone();
        
        // Check if token exists
        let found = {
            let tracker_guard = token_tracker.read().unwrap();
            tracker_guard.is_token_tracked(token_mint).await
        };
        
        if !found {
            return false;
        }
        
        // Check all other filters
        let passes = {
            let _tracker_guard = token_tracker.read().unwrap();
            
            // Logic to check token criteria goes here
            true // Placeholder - replace with actual logic
        };
        
        passes
    }

    // Identify dev wallet for a token
    async fn identify_dev_wallet(&self, _token_mint: &str) -> Option<String> {
        let _tracker = self.token_tracker.read().unwrap();
        
        // Implementation details to identify dev wallet
        // This could include analyzing transaction patterns, checking first creator, etc.
        
        // Placeholder implementation - replace with actual logic
        Some("0xDEV_WALLET_ADDRESS".to_string())
    }
}

// Event definitions for stream processing
pub enum StreamEvent {
    TokenMint {
        token_mint: String,
        dev_buy_amount: f64,
        launcher_sol_balance: f64,
        token_price: f64,
        bundle_check: bool,
    },
    TokenTransaction {
        token_mint: String,
        is_buy: bool,
        amount: f64,
        price: f64,
    },
}

pub fn process_tokens(tracker: Arc<RwLock<TokenTracker>>) {
    tokio::spawn(async move {
        // Process daily tasks
        loop {
            // Get logger and clone it before any async operations
            let logger = {
                let guard = tracker.read().unwrap();
                guard.logger.clone()
            };
            
            // Clean expired tokens without holding locks across await
            {
                // Clone all fields we need from the tracker
                let tokens_arc = {
                    let guard = tracker.read().unwrap();
                    guard.tokens.clone()
                };
                
                let extended_tokens_arc = {
                    let guard = tracker.read().unwrap();
                    guard.extended_tokens.clone()
                };
                
                let time_threshold = {
                    let guard = tracker.read().unwrap();
                    guard.time_threshold
                };
                
                let price_check_threshold = {
                    let guard = tracker.read().unwrap();
                    guard.price_check_threshold
                };
                
                let filter_time_delta = {
                    let guard = tracker.read().unwrap();
                    guard.filter.time_delta_threshold
                };
                
                let logger_clone = logger.clone();
                
                // Identify tokens to remove
                let mut tokens_to_remove = Vec::new();
                {
                    let tokens = tokens_arc.read().unwrap();
                    for (mint, token) in tokens.iter() {
                        // Check time threshold
                        if token.last_updated.elapsed() > time_threshold {
                            tokens_to_remove.push(mint.clone());
                            continue;
                        }
                        
                        // Check price delta
                        if let Some(change) = token.calculate_price_change(Duration::from_secs(filter_time_delta)) {
                            if change < -price_check_threshold {
                                tokens_to_remove.push(mint.clone());
                                continue;
                            }
                        }
                    }
                }
                
                // Process tokens to remove without holding locks across await points
                for mint in tokens_to_remove {
                    // Cancel tasks
                    {
                        let mut extended_tokens = extended_tokens_arc.lock().unwrap();
                        if let Some(ext_token) = extended_tokens.get_mut(&mint) {
                            if let Some(task) = ext_token.active_task.take() {
                                task.abort();
                            }
                        }
                    }
                    
                    // Remove token from tracking
                    {
                        let mut tokens = tokens_arc.write().unwrap();
                        if tokens.remove(&mint).is_some() {
                            logger_clone.log(format!(
                                "[TOKEN REMOVED] => No longer tracking: {}", 
                                mint
                            ).yellow().to_string());
                        }
                    }
                }
            }
            
            // Count tokens without holding locks across await
            let token_count = {
                let guard = tracker.read().unwrap();
                let tokens = guard.tokens.read().unwrap();
                tokens.len()
            };
            
            // Log the count (no async operations here)
            logger.log(format!("Current token count: {}", token_count));
            
            // Wait for 1 hour before next check
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
} 