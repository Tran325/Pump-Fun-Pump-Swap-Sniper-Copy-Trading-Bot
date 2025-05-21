// Enhanced token trader with time series analysis and advanced monitoring
use std::sync::{Arc, Mutex};
use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;

use anchor_client::solana_sdk::signature::Signer;

use crate::common::config::{LiquidityPool, Config};
use crate::common::logger::Logger;
use crate::services::telegram::TelegramService;
use crate::engine::token_tracker::{TokenTracker, TokenFilter};
use crate::engine::token_buying::BuyManager;
use crate::engine::token_selling::SellManager;

use crate::engine::enhanced_monitor::EnhancedMonitor;
use crate::engine::advanced_trading::{integration as advanced_trading_integration};
use crate::engine::bonding_curve::BondingCurveManager;
use crate::engine::risk_management::{self as risk_mgmt};
use crate::dex::pump_fun::Pump;

/// Starts the enhanced token trader with specified parameters for filtering and trading tokens
///
/// # Arguments
/// * `config` - The configuration for the application
/// * `min_launcher_sol` - Minimum SOL balance for token launchers to track
/// * `max_launcher_sol` - Maximum SOL balance for token launchers to track
/// * `min_market_cap` - Minimum market cap to consider for trading
/// * `max_market_cap` - Maximum market cap to consider for trading
/// * `min_volume` - Minimum trading volume to consider for trading
/// * `max_volume` - Maximum trading volume to consider for trading
/// * `min_buy_sell_count` - Minimum number of buy/sell transactions to consider for trading
/// * `max_buy_sell_count` - Maximum number of buy/sell transactions to consider for trading
/// * `price_delta_threshold` - Price change percentage threshold for triggering alerts
/// * `time_delta_threshold` - Time threshold in seconds for monitoring price changes
pub async fn start(
    config: Config,
    min_launcher_sol: f64,
    max_launcher_sol: f64,
    min_market_cap: f64,
    max_market_cap: f64,
    min_volume: f64,
    max_volume: f64,
    min_buy_sell_count: u32,
    max_buy_sell_count: u32,
    price_delta_threshold: f64,
    time_delta_threshold: u64,
) -> Result<()> {
    // Set up logger
    let logger = Logger::new("[ENHANCEDTRADER] => ".green().bold().to_string());
    logger.log("Initializing enhanced token trader with advanced strategies...".green().to_string());
    
    // Initialize PumpFun swap client
    let _swap_config = config.swap_config.clone();
    let rpc_nonblocking_client = config.app_state.rpc_nonblocking_client.clone();
    let rpc_client = config.app_state.rpc_client.clone();
    let keypair = config.app_state.wallet.clone();
    let swapx = Pump::new(
        rpc_nonblocking_client,
        rpc_client,
        keypair
    );
    
    // Initialize token filter
    let _token_filter = TokenFilter {
        min_launcher_sol,
        max_launcher_sol,
        min_market_cap,
        max_market_cap,
        min_volume,
        max_volume,
        min_buy_sell_count,
        max_buy_sell_count,
        price_delta_threshold,
        time_delta_threshold,
    };
    
    // Initialize token tracker
    let existing_liquidity_pools = Arc::new(Mutex::new(HashSet::<LiquidityPool>::new()));
    let token_tracker = Arc::new(TokenTracker::new(
        logger.clone(),
        price_delta_threshold,
        time_delta_threshold,
        min_market_cap,
        max_market_cap,
        min_volume,
        max_volume,
        min_buy_sell_count,
        max_buy_sell_count,
        min_launcher_sol,
        max_launcher_sol
    ));
    
    // Initialize sell manager
    let sell_manager = Arc::new(Mutex::new(SellManager::new(logger.clone())));
    
    // Initialize buy manager (new)
    let _buy_manager = Arc::new(Mutex::new(BuyManager::new(logger.clone())));
    
    // Initialize advanced trading systems
    let _bonding_curve_mgr = Arc::new(Mutex::new(
        BondingCurveManager::new(logger.clone())
    ));
    
    // Get portfolio value from wallet balance
    let wallet_balance = config.app_state.rpc_client.get_balance(&config.app_state.wallet.pubkey()).unwrap_or(0);
    let portfolio_value = wallet_balance as f64 / 1_000_000_000.0; // Convert lamports to SOL
    
    // Initialize risk management system
    let _risk_manager = risk_mgmt::start_risk_management_system(
        logger.clone(),
        portfolio_value
    ).await;
    
    // Set up telegram service if configured
    let telegram_service = if !config.telegram_bot_token.is_empty() && !config.telegram_chat_id.is_empty() {
        let telegram = TelegramService::new(
            config.telegram_bot_token.clone(),
            config.telegram_chat_id.clone(),
            30 // Rate limit notifications to 1 per 30 seconds
        );
        Some(Arc::new(telegram))
    } else {
        None
    };
    
    // Start advanced trading system with buy capability
    advanced_trading_integration::start_advanced_trading_system(
        token_tracker.clone(),
        sell_manager.clone(),
        swapx.clone(),
        existing_liquidity_pools.clone(),
        telegram_service.clone(),
        logger.clone(),
        portfolio_value
    ).await?;
    
    // Create a Config implementation that can be cloned
    #[derive(Clone)]
    struct ConfigWrapper {
        inner: Arc<Config>
    }
    
    impl std::ops::Deref for ConfigWrapper {
        type Target = Config;
        
        fn deref(&self) -> &Self::Target {
            &*self.inner
        }
    }
    
    let config_wrapper = ConfigWrapper {
        inner: Arc::new(config.clone())
    };
    
    // Initialize enhanced monitor with buy manager - dereference ConfigWrapper to get Config
    let monitor = EnhancedMonitor::new(
        (*config_wrapper).clone(), // Dereference and clone the Config
        token_tracker.clone(),
        sell_manager.clone(),
        swapx.clone(),
        existing_liquidity_pools.clone(),
        telegram_service.clone(),
        config_wrapper.inner.telegram_chat_id.clone(),
        logger.clone(),
    ).await?;
    
    // Start monitoring
    monitor.start().await?;
    
    Ok(())
} 