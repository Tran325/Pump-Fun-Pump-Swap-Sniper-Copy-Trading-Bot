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
