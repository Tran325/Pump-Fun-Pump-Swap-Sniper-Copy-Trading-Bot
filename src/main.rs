use solana_vntr_sniper::{
    common::{config::Config, constants::RUN_MSG},
    engine::monitor::new_token_trader_pumpfun,
    services::telegram::{TelegramService, TelegramFilterSettings},
    tests::run_dev_wallet_test,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task;
use chrono;
use std::env;

#[tokio::main]
async fn main() {
    // Check if we should run the dev wallet test
    let args: Vec<String> = env::args().collect();
    
    // If the "--test-dev-wallet" argument is passed, run the test and exit
    if args.len() > 1 && args[1] == "--test-dev-wallet" {
        println!("Running dev wallet detection test...");
        if let Err(e) = run_dev_wallet_test().await {
            eprintln!("Error running dev wallet test: {}", e);
            std::process::exit(1);
        }
        println!("Dev wallet test completed successfully!");
        std::process::exit(0);
    }

    // Check if enhanced mode is enabled
    let use_enhanced_mode = std::env::var("USE_ENHANCED_MODE").unwrap_or_else(|_| "false".to_string()) == "true";

    /* Initial Settings */
    let config = Config::new().await;
    let config = config.lock().await;

    /* Running Bot */
    let run_msg = RUN_MSG;
    println!("{}", run_msg);
    
    if use_enhanced_mode {
        println!("🚀 Starting in ENHANCED mode with time series analysis and advanced trading strategies");
    } else {
        println!("🚀 Starting in STANDARD mode with basic monitoring");
    }

    // Log info about whitelist and blacklist functionality
    println!("Token list features enabled:");
    println!(" - Whitelist auto-management: Active tokens kept every review cycle");
    println!(" - Blacklist protection: Blacklisted tokens completely ignored");
    println!(" - Review cycle: {} seconds", 
        std::env::var("REVIEW_CYCLE_MS").unwrap_or_else(|_| "120000".to_string()).parse::<u64>().unwrap_or(120000) / 1000);
    println!(" - Lists save interval: {} minutes",
        std::env::var("SAVE_INTERVAL_MS").unwrap_or_else(|_| "600000".to_string()).parse::<u64>().unwrap_or(600000) / 60000);

    // Send telegram notification with bot configuration if Telegram is enabled
    if !config.telegram_bot_token.is_empty() && !config.telegram_chat_id.is_empty() {
        // Create Telegram service with improved notification system
        // Note: The notification_interval parameter (30 seconds) helps rate limit notifications
        // and the service now includes deduplication to prevent multiple notifications for the same token
        let telegram_service = TelegramService::new(
            config.telegram_bot_token.clone(),
            config.telegram_chat_id.clone(),
            30 // Rate limit notifications to 1 per 30 seconds
        );
        
        // Get filter settings
        let filter_settings = TelegramFilterSettings::from_env();

        // Get take profit and stop loss status from env
        let take_profit_enabled = std::env::var("TAKE_PROFIT").unwrap_or_else(|_| "false".to_string()) == "true";
        let stop_loss_enabled = std::env::var("STOP_LOSS").unwrap_or_else(|_| "false".to_string()) == "true";
        let tp_status = if take_profit_enabled { format!("Enabled ({}%)", config.take_profit_percent) } else { "Disabled".to_string() };
        let sl_status = if stop_loss_enabled { format!("Enabled ({}%)", config.stop_loss_percent) } else { "Disabled".to_string() };

        // Get review cycle and save interval
        let review_cycle_seconds = std::env::var("REVIEW_CYCLE_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120000) / 1000;
            
        let save_interval_minutes = std::env::var("SAVE_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600000) / 60000;

        // Create configuration message
        let config_message = format!(
            "<b>🤖 BOT STARTED - CONFIGURATION</b>\n\n\
            <b>🔹 Mode:</b> {}\n\n\
            <b>🔹 General Settings:</b>\n\
            ├ Token Amount: {} SOL\n\
            ├ Slippage: {}%\n\
            ├ Time Exceed: {} seconds\n\
            ├ Counter Limit: {}\n\
            ├ Use Jito: {}\n\
            └ Bundle Check: {}\n\n\
            <b>🔹 Dev Buy Settings:</b>\n\
            ├ Min Dev Buy: {} SOL\n\
            └ Max Dev Buy: {} SOL\n\n\
            <b>🔹 Profit/Loss Settings:</b>\n\
            ├ Take Profit: {}\n\
            └ Stop Loss: {}\n\n\
            <b>🔹 Token List Management:</b>\n\
            ├ Whitelist: Enabled (active tokens kept)\n\
            ├ Blacklist: Enabled (tokens skipped)\n\
            ├ Review Cycle: {} seconds\n\
            └ Save Interval: {} minutes\n\n\
            <b>🔹 Filters Enabled:</b>\n\
            ├ Market Cap: {} (Range: {:.1}-{:.1}K)\n\
            ├ Volume: {} (Range: {:.1}-{:.1}K)\n\
            ├ Buy/Sell Count: {} (Range: {}-{})\n\
            ├ SOL Invested: {} (Min: {:.1} SOL)\n\
            ├ Launcher Balance: {} (Range: {:.1}-{:.1} SOL)\n\
            └ Dev Buy: {} (Range: {:.1}-{:.1} SOL)\n\n\
            <b>🔹 New Features:</b>\n\
            ├ Dev Wallet Detection: Enabled\n\
            └ Notification Deduplication: Enabled",
            if use_enhanced_mode { "ENHANCED (with time series analysis)" } else { "STANDARD" },
            config.swap_config.amount_in,
            config.swap_config.slippage,
            config.time_exceed,
            config.counter_limit,
            config.swap_config.use_jito,
            config.bundle_check,
            config.min_dev_buy,
            config.max_dev_buy,
            tp_status,
            sl_status,
            review_cycle_seconds,
            save_interval_minutes,
            // Filter settings
            filter_settings.market_cap_enabled,
            filter_settings.market_cap.min,
            filter_settings.market_cap.max,
            filter_settings.volume_enabled,
            filter_settings.volume.min,
            filter_settings.volume.max,
            filter_settings.buy_sell_count_enabled,
            filter_settings.buy_sell_count.min,
            filter_settings.buy_sell_count.max,
            filter_settings.sol_invested_enabled,
            filter_settings.sol_invested,
            filter_settings.launcher_sol_balance_enabled,
            filter_settings.launcher_sol_balance.min,
            filter_settings.launcher_sol_balance.max,
            filter_settings.dev_buy_bundle_enabled,
            filter_settings.dev_buy_bundle.min,
            filter_settings.dev_buy_bundle.max
        );

        // Send configuration message
        if let Err(e) = telegram_service.send_message(&config.telegram_chat_id, &config_message, "HTML").await {
            eprintln!("Failed to send configuration notification: {}", e);
        }
        
        // Start periodic status update task
        let telegram_service = Arc::new(telegram_service);
        let telegram_chat_id = config.telegram_chat_id.clone();
        
        // Clone the parts of config we need for the status update task
        let time_exceed = config.time_exceed;
        let counter_limit = config.counter_limit;
        
        // Set the status update interval (7 minutes = 420 seconds)
        // This interval determines how often the bot sends Telegram status updates
        let status_update_interval = Duration::from_secs(420);
        
        task::spawn(async move {
            let mut last_update = Instant::now();
            let start_time = Instant::now();
            
            loop {
                // Sleep for 1 minute before checking if we need to send an update
                tokio::time::sleep(Duration::from_secs(60)).await;
                
                // Check if it's time to send an update
                if last_update.elapsed() >= status_update_interval {
                    // Get current time
                    let current_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    
                    // Get the current filter settings from the telegram service
                    let filter_settings = telegram_service.get_filter_settings();
                    
                    // Create status message
                    let status_message = format!(
                        "<b>🔄 BOT STATUS UPDATE</b> - {}\n\n\
                        <b>✅ Bot is running</b>\n\
                        <b>⏱️ Uptime:</b> {} minutes\n\
                        <b>🔎 Monitoring for:</b>\n\
                        ├ Dev Buy Range: {}-{} SOL\n\
                        ├ Time Exceed: {} seconds\n\
                        └ Counter Limit: {}\n\n\
                        <b>📊 Current Filters:</b>\n\
                        ├ Market Cap: {} (Range: {:.1}-{:.1}K)\n\
                        ├ Volume: {} (Range: {:.1}-{:.1}K)\n\
                        ├ Buy/Sell Count: {} (Range: {}-{})\n\
                        ├ SOL Invested: {} (Min: {:.1} SOL)\n\
                        ├ Launcher Balance: {} (Range: {:.1}-{:.1} SOL)\n\
                        └ Dev Buy: {} (Range: {:.1}-{:.1} SOL)\n\n\
                        <b>📊 Notification Stats:</b>\n\
                        ├ Unique Tokens Notified: {}\n\n\
                        <i>This is an automated status update. Bot continues to monitor for token opportunities.</i>",
                        current_time,
                        start_time.elapsed().as_secs() / 60,
                        filter_settings.dev_buy_bundle.min,
                        filter_settings.dev_buy_bundle.max,
                        time_exceed,
                        counter_limit,
                        // Filter settings from the telegram service's persisted config
                        filter_settings.market_cap_enabled,
                        filter_settings.market_cap.min,
                        filter_settings.market_cap.max,
                        filter_settings.volume_enabled,
                        filter_settings.volume.min,
                        filter_settings.volume.max,
                        filter_settings.buy_sell_count_enabled,
                        filter_settings.buy_sell_count.min,
                        filter_settings.buy_sell_count.max,
                        filter_settings.sol_invested_enabled,
                        filter_settings.sol_invested,
                        filter_settings.launcher_sol_balance_enabled,
                        filter_settings.launcher_sol_balance.min,
                        filter_settings.launcher_sol_balance.max,
                        filter_settings.dev_buy_bundle_enabled,
                        filter_settings.dev_buy_bundle.min,
                        filter_settings.dev_buy_bundle.max,
                        // Show number of unique tokens that have been notified
                        telegram_service.get_notified_tokens().len()
                    );
                    
                    // Send status message
                    if let Err(e) = telegram_service.send_message(&telegram_chat_id, &status_message, "HTML").await {
                        eprintln!("Failed to send status update notification: {}", e);
                    } else {
                        // Update the last update time
                        last_update = Instant::now();
                        println!("Sent periodic status update via Telegram");
                    }
                }
            }
        });
    }

        if let Err(e) = new_token_trader_pumpfun(
            config.yellowstone_grpc_http.clone(),
            config.yellowstone_grpc_token.clone(),
            config.yellowstone_ping_interval,
            config.yellowstone_reconnect_delay,
            config.yellowstone_max_retries,
            config.app_state.clone(),
            config.swap_config.clone(),
            config.blacklist.clone(),
            config.time_exceed,
            config.counter_limit as u64,
            config.min_dev_buy as u64,
            config.max_dev_buy as u64,
            config.telegram_bot_token.clone(),
            config.telegram_chat_id.clone(),
            config.bundle_check,
            config.min_last_time,
        ).await {
            eprintln!("Standard token trader error: {}", e);
    }

    // Add transaction notification monitor
    if !config.telegram_bot_token.is_empty() && !config.telegram_chat_id.is_empty() {
        // Create a dedicated Telegram service for transaction notifications
        let _tx_notification_service = Arc::new(TelegramService::new(
            config.telegram_bot_token.clone(),
            config.telegram_chat_id.clone(),
            5 // Lower rate limit for transaction notifications
        ));
        
        println!("📱 Transaction notification system initialized");
        // This will be handled by the existing monitoring systems that now use
        // the new send_transaction_notification method we've implemented
    }

    // Keep the main thread alive
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
