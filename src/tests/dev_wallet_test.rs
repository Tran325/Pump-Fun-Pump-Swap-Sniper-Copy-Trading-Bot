use std::sync::Arc;
use crate::engine::token_tracker::{TokenTracker, StreamEvent};
use crate::common::logger::Logger;
use crate::services::telegram::TelegramService;
use colored::Colorize;
use tokio::time::Duration;
use std::env;

/// Runs a test of the dev wallet identification and notification deduplication features
/// 
/// This function will:
/// 1. Create a token tracker
/// 2. Add a test token to the tracker
/// 3. Simulate transactions for the token
/// 4. Try to identify the dev wallet
/// 5. Send a notification via Telegram (if credentials are available)
/// 6. Test notification deduplication by trying to send another notification for the same token
pub async fn run_dev_wallet_test() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger
    let logger = Logger::new("[TEST DEV WALLET] => ".green().bold().to_string());
    logger.log("Starting test script for dev wallet identification".to_string());
    
    // Load environment variables for Telegram
    let telegram_bot_token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let telegram_chat_id = env::var("TELEGRAM_CHAT_ID").unwrap_or_default();
    
    // Initialize Telegram service if credentials are provided
    let telegram_service = if !telegram_bot_token.is_empty() && !telegram_chat_id.is_empty() {
        let service = TelegramService::new(telegram_bot_token.clone(), telegram_chat_id.clone(), 1);
        Some(Arc::new(service))
    } else {
        logger.log("Telegram credentials not provided, notifications will be disabled".red().to_string());
        None
    };
    
    // Initialize token tracker with default parameters
    let token_tracker = Arc::new(TokenTracker::new(
        logger.clone(),
        10.0,  // price_check_threshold
        3600,  // time_threshold_secs
        1.0,   // min_market_cap
        1000.0, // max_market_cap
        1.0,   // min_volume
        1000.0, // max_volume
        5,     // min_buy_sell_count
        1000,  // max_buy_sell_count
        0.1,   // min_launcher_sol_balance
        100.0  // max_launcher_sol_balance
    ));
    
    // Create a simulated token mint event
    let test_token_mint = format!("TestToken{}", chrono::Utc::now().timestamp());
    let _stream_event = StreamEvent::TokenMint {
        token_mint: test_token_mint.clone(),
        dev_buy_amount: 10.0,         // Dev bought 10 SOL worth
        launcher_sol_balance: 5.0,    // Launcher has 5 SOL
        token_price: 0.001,           // Initial token price 
        bundle_check: true,           // Bundle check passed
    };
    
    logger.log(format!("Simulating token mint event for token: {}", test_token_mint).cyan().to_string());
    
    // Add the token to the tracker
    let _token = token_tracker.add_token(
        test_token_mint.clone(),
        0.001, // Initial price 
        5.0,   // Launcher SOL
        Some(10.0), // Dev buy amount 
        Some(5.0),  // Launcher SOL balance
        Some(true), // Bundle check
        Some("TEST".to_string()), // Token name
        Some("TST".to_string()),  // Token symbol
    ).await?;
    
    logger.log("Token added to tracker".green().to_string());
    
    // Wait a moment for processing
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Simulate token transactions to increase buy/sell count
    for i in 0..10 {
        let is_buy = i % 3 != 0; // 2/3 are buys, 1/3 are sells
        let amount = if is_buy { 0.5 } else { 0.3 };
        
        logger.log(format!(
            "Simulating {} transaction #{} for {} SOL",
            if is_buy { "BUY" } else { "SELL" },
            i + 1,
            amount
        ).cyan().to_string());
        
        token_tracker.update_token(
            &test_token_mint,
            amount,
            is_buy
        ).await?;
        
        // Short delay between transactions
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    
    // Get the token information from the tracker
    if let Some(token_info) = token_tracker.get_token(&test_token_mint).await {
        logger.log(format!(
            "Token {} has {} buys and {} sells",
            test_token_mint,
            token_info.buy_count,
            token_info.sell_count
        ).green().to_string());
        
        // Get extended token info
        let extended_tokens = token_tracker.get_extended_tokens_map_arc();
        let extended_token_info = {
            let guard = extended_tokens.lock().unwrap();
            guard.get(&test_token_mint).map(|token| token.clone_without_task())
        };
        
        if let Some(ext_info) = extended_token_info {
            logger.log(format!(
                "Extended token info: dev wallet = {}",
                ext_info.dev_wallet.as_ref().unwrap_or(&"Not identified".to_string())
            ).green().to_string());
            
            // Send notification via Telegram if available
            if let Some(telegram) = &telegram_service {
                logger.log("Sending Telegram notification...".to_string());
                
                let telegram_token_info = ext_info.to_telegram_token_info();
                
                match telegram.send_token_notification(&telegram_token_info).await {
                    Ok(_) => logger.log("Telegram notification sent successfully".green().to_string()),
                    Err(e) => logger.log(format!("Failed to send Telegram notification: {}", e).red().to_string()),
                };
                
                // Wait a moment for the notification to be processed
                tokio::time::sleep(Duration::from_secs(2)).await;
                
                // Try to send another notification for the same token - this should be prevented
                logger.log("Attempting to send another notification for the same token...".yellow().to_string());
                match telegram.send_token_notification(&telegram_token_info).await {
                    Ok(_) => logger.log("Second notification sent - duplicate prevention failed!".red().to_string()),
                    Err(e) => logger.log(format!("Second notification properly blocked: {}", e).green().to_string()),
                };
            }
        } else {
            logger.log("Extended token info not found".red().to_string());
        }
    } else {
        logger.log("Token not found in tracker".red().to_string());
    }
    
    logger.log("Test script completed".green().to_string());
    Ok(())
} 