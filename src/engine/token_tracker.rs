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
