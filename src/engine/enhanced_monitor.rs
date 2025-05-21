use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::str::FromStr;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::hash::Hash as SolanaHash;
use spl_token::solana_program::native_token::{lamports_to_sol, LAMPORTS_PER_SOL};
use chrono::Utc;
use colored::Colorize;
use futures_util::StreamExt;
use tokio::time::{self, Instant};
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient, InterceptorXToken};
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestPing,
    SubscribeRequestFilterTransactions, SubscribeUpdateTransaction
};
use anyhow::{anyhow, Result};
use maplit::hashmap;
use futures::channel::mpsc as futures_mpsc;
use base64;

// Add an explicit import with 'as' to avoid any ambiguity with the log crate
use crate::common::logger::Logger;

use crate::common::{
    blacklist::Blacklist,
    config::{AppState, LiquidityPool, SwapConfig, JUPITER_PROGRAM, OKX_DEX_PROGRAM, Config},
};
use crate::dex::pump_fun::{Pump, PUMP_FUN_CREATE_IX_DISCRIMINATOR, PUMP_PROGRAM};
use crate::services::telegram::{TelegramService, TokenInfo};
use crate::engine::token_tracker::{TokenTracker, ExtendedTokenInfo};
use crate::engine::token_selling::SellManager;
use crate::engine::token_buying::BuyManager;
use crate::engine::token_list_manager::{TokenListManager};
use crate::engine::monitor::{ParsedTransactionInfo, TransactionType, FilterConfig, PUMPFUN_CREATE_DATA_PREFIX as MONITOR_PUMPFUN_CREATE_DATA_PREFIX, PUMP_FUN_BUY_OR_SELL_PROGRAM_DATA_PREFIX, BondingCurveInfo};

// Function to create a new gRPC client and subscription
async fn create_grpc_client(
    yellowstone_grpc_http: &str, 
    yellowstone_grpc_token: &str,
    filter_config: &FilterConfig,
    logger: &Logger,
) -> Result<(GeyserGrpcClient<InterceptorXToken>, tokio::sync::mpsc::UnboundedSender<SubscribeRequest>, tokio::sync::mpsc::Receiver<yellowstone_grpc_proto::geyser::SubscribeUpdate>), String> {
    logger.log("Connecting to Yellowstone gRPC...".to_string());
    
    // Create client
    let builder = match GeyserGrpcClient::build_from_shared(yellowstone_grpc_http.to_owned()) {
        Ok(builder) => builder,
        Err(e) => return Err(format!("Failed to create builder: {}", e))
    };
    
    // Add token if specified
    let builder = if !yellowstone_grpc_token.is_empty() {
        match builder.x_token(Some(yellowstone_grpc_token.to_owned())) {
            Ok(builder) => builder,
            Err(e) => return Err(format!("Failed to set token: {}", e))
        }
    } else {
        builder
    };
    
    // Set up TLS configuration
    let builder = match builder.tls_config(ClientTlsConfig::new().with_native_roots()) {
        Ok(builder) => builder,
        Err(e) => return Err(format!("Failed to set TLS config: {}", e))
    };

    // Connect the client
    let mut client = match builder.connect_lazy() {
        Ok(client) => client,
        Err(e) => return Err(format!("Failed to connect: {}", e))
    };

    // Create a subscription - using futures_mpsc which implements Stream
    let (subscribe_tx_futures, subscribe_rx_futures) = futures_mpsc::unbounded();
    
    // Create initial subscription request
    let request = SubscribeRequest {
        slots: HashMap::new(),
        accounts: HashMap::new(),
        transactions: hashmap! {
            "All".to_owned() => SubscribeRequestFilterTransactions {
                vote: None,
                failed: Some(false),
                signature: None,
                account_include: filter_config.program_ids.clone(),
                account_exclude: vec![JUPITER_PROGRAM.to_string(), OKX_DEX_PROGRAM.to_string()],
                account_required: Vec::<String>::new()
            }
        },
        transactions_status: HashMap::new(),
        entry: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::new(),
        commitment: Some(CommitmentLevel::Processed as i32),
        accounts_data_slice: vec![],
        ping: None,
        from_slot: None,
    };
    
    // Send initial request using futures channel
    if let Err(e) = subscribe_tx_futures.unbounded_send(request) {
        return Err(format!("Failed to create subscribe request: {}", e));
    }
    
    // Subscribe using the futures stream
    let response = match client.geyser.subscribe(subscribe_rx_futures).await {
        Ok(response) => response,
        Err(e) => return Err(format!("Failed to subscribe: {}", e))
    };
    
    let streaming = response.into_inner();
    
    // Create Tokio versions for compatibility with the rest of the code
    let (update_tx, update_rx) = tokio::sync::mpsc::channel(100);
    let (subscribe_tx, _subscribe_rx) = tokio::sync::mpsc::unbounded_channel();
    
    // Spawn a task to forward updates from the streaming response to the Tokio channel
    tokio::spawn(async move {
        let mut stream = streaming;
        while let Some(update) = stream.next().await {
            if let Ok(update) = update {
                if update_tx.send(update).await.is_err() {
                    break;
                }
            }
        }
    });
    
    logger.log("Successfully connected to Yellowstone gRPC".to_string());
    
    // We'll convert the InterceptorXToken type to ensure type compatibility with the function signature
    let client_typed = unsafe { std::mem::transmute(client) };
    
    Ok((client_typed, subscribe_tx, update_rx))
}

/// Enhanced version of the token trader function that uses our token tracker
pub async fn enhanced_token_trader(
    yellowstone_grpc_http: String,
    yellowstone_grpc_token: String,
    yellowstone_ping_interval: u64,
    yellowstone_reconnect_delay: u64,
    _yellowstone_max_retries: u32,
    app_state: AppState,
    swap_config: SwapConfig,
    _blacklist: Blacklist,
    _time_exceed: u64,
    _counter_limit: u64,
    min_dev_buy: u64,
    max_dev_buy: u64,
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
    telegram_bot_token: String,
    telegram_chat_id: String,
    bundle_check: bool,
    take_profit_percent: f64,
    stop_loss_percent: f64,
    _min_last_time: u64,
) -> Result<(), String> {
    let logger = Arc::new(Logger::new("[ENHANCED-MONITOR] => ".blue().bold().to_string()));
    
    // Create additional clones for later use in tasks
    let yellowstone_grpc_http = Arc::new(yellowstone_grpc_http);
    let yellowstone_grpc_token = Arc::new(yellowstone_grpc_token);
    let app_state = Arc::new(app_state);
    let swap_config = Arc::new(swap_config);
    
    // Load LIMIT_WAIT_TIME and LIMIT_BUY_AMOUNT_IN_LIMIT_WAIT_TIME from environment
    let limit_wait_time = std::env::var("LIMIT_WAIT_TIME")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(120000); // Default to 120 seconds (120,000 ms)
    
    let limit_buy_amount_in_limit_wait_time = std::env::var("LIMIT_BUY_AMOUNT_IN_LIMIT_WAIT_TIME")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(15.0); // Default to 15 SOL
    // Create our token tracker
    let token_tracker = Arc::new(Mutex::new(TokenTracker::new(
        (*logger).clone(),
        price_delta_threshold,
        time_delta_threshold,
        min_market_cap,
        max_market_cap,
        min_volume,
        max_volume,
        min_buy_sell_count,
        max_buy_sell_count,
        min_launcher_sol,
        max_launcher_sol,
    )));
    
    // Load REVIEW_CYCLE_MS from environment or use default 120,000 ms (2 minutes)
    let review_cycle_ms = std::env::var("REVIEW_CYCLE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(120000); // Default to 2 minutes
        
    // Load SAVE_INTERVAL_MS from environment or use default 600,000 ms (10 minutes)
    let save_interval_ms = std::env::var("SAVE_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600000); // Default to 10 minutes
    
    // Create token list manager for whitelist/blacklist
    let token_list_manager = match TokenListManager::new(
        "./whitelist.json", 
        "./blacklist.json", 
        review_cycle_ms,
        save_interval_ms,
        get_inner_logger(&logger)
    ) {
        Ok(manager) => Arc::new(manager),
        Err(e) => {
            logger.log(format!("Failed to initialize token list manager: {}", e).red().to_string());
            return Err(format!("Failed to initialize token list manager: {}", e));
        }
    };
    
    // Start background tasks for token list manager
    let token_list_manager_clone = token_list_manager.clone();
    tokio::spawn(async move {
        token_list_manager_clone.start_background_tasks();
    });
    
    // Log review cycle configuration
    logger.log(format!(
        "🔄 Token review cycle: {} ms | 💾 Save interval: {} ms",
        review_cycle_ms,
        save_interval_ms
    ).yellow().to_string());
    
    // Create sell manager
    let sell_manager = Arc::new(Mutex::new(SellManager::new(get_inner_logger(&logger))));

    // Create buy manager
    let buy_manager = Arc::new(Mutex::new(BuyManager::new(get_inner_logger(&logger))));

    // Log the take profit and stop loss settings
    logger.log(format!(
        "Take Profit: {}% | Stop Loss: {}%",
        take_profit_percent.to_string().green(),
        stop_loss_percent.to_string().red()
    ).yellow().to_string());

    // Initialize Telegram service if credentials are provided
    let telegram_service = if !telegram_bot_token.is_empty() && !telegram_chat_id.is_empty() {
        let service = TelegramService::new(telegram_bot_token, telegram_chat_id.clone(), 30);
        
        // Send initial filter UI
        if let Err(e) = service.send_filter_settings_ui().await {
            logger.log(format!("Failed to send Telegram filter UI: {}", e).red().to_string());
        }
        
        // Start polling for Telegram updates
        let service_clone = service.clone();
        tokio::spawn(async move {
            service_clone.start_polling().await;
        });
        
        Some(Arc::new(service))
    } else {
        None
    };

    // Set up filter config - IMPORTANT: This controls which transactions we receive
    let filter_config = FilterConfig {
        program_ids: vec![PUMP_PROGRAM.to_string()],
        _instruction_discriminators: (&[PUMP_FUN_CREATE_IX_DISCRIMINATOR]).to_vec(),
    };
    // Create shared pools collection (for backward compatibility)
    let existing_liquidity_pools = Arc::new(Mutex::new(HashSet::<LiquidityPool>::new()));
    // Set up swap client
    let rpc_nonblocking_client = app_state.clone().rpc_nonblocking_client.clone();
    let rpc_client = app_state.clone().rpc_client.clone();
    let wallet = app_state.clone().wallet.clone();
    let swapx = Pump::new(
        rpc_nonblocking_client.clone(),
        rpc_client.clone(),
        wallet.clone(),
    );

    // Start token cleanup task to remove tokens that don't meet the activity criteria
    let token_tracker_activity_cleanup: Arc<Mutex<TokenTracker>> = Arc::clone(&token_tracker);
    let activity_cleanup_logger = logger.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60)); // Check every minute
        loop {
            interval.tick().await;
            // Lock the token tracker
            let tracker = match token_tracker_activity_cleanup.lock() {
                Ok(tracker) => tracker,
                Err(e) => {
                    activity_cleanup_logger.log(format!("Failed to lock token tracker: {}", e).red().to_string());
                    continue;
                }
            };

            // Get a list of all tracked tokens
            let extended_tokens = tracker.get_extended_tokens_map_arc();
            let mut tokens_to_remove = Vec::new();
            // Check each token's activity - use a separate scope to ensure the mutex guard is dropped
            {
                if let Ok(tokens_map) = extended_tokens.lock() {
                    for (token_mint, token_info) in tokens_map.iter() {
                        // Get token lifetime in milliseconds
                        let token_lifetime_ms = token_info.token_mint_timestamp.elapsed().as_millis() as u64;
                        // Skip tokens that haven't existed long enough to evaluate
                        if token_lifetime_ms < limit_wait_time {
                            continue;
                        }
                        // Check if the token has sufficient buy activity
                        if let Some(volume) = token_info.volume {
                            if volume < limit_buy_amount_in_limit_wait_time && token_info.buy_tx_num < min_buy_sell_count {
                                activity_cleanup_logger.log(
                                    format!(
                                        "Removing token {} due to insufficient activity: {} SOL volume, {} buy txs during {} ms window",
                                        token_mint, volume, token_info.buy_tx_num, limit_wait_time
                                    ).yellow().to_string()
                                );
                                tokens_to_remove.push(token_mint.clone());
                            }
                        }
                    }
                }
            }
            // Remove the tokens that didn't meet activity criteria
            for token_mint in tokens_to_remove {
                tracker.stop_token_monitoring(&token_mint);
            }
        }
    });
    // Start token activity recording task
    let token_tracker_activity_recorder: Arc<Mutex<TokenTracker>> = Arc::clone(&token_tracker);
    let activity_recorder_logger = logger.clone();

    tokio::spawn(async move {
        // Create directory for token activity data if it doesn't exist
        let data_dir = "token_activity_data";
        if !std::path::Path::new(data_dir).exists() {
            if let Err(e) = std::fs::create_dir_all(data_dir) {
                activity_recorder_logger.log(format!("Failed to create token activity data directory: {}", e).red().to_string());
            }
        }
        
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // Save data every 5 minutes
        
        loop {
            interval.tick().await;
            
            // Lock the token tracker
            let tracker = match token_tracker_activity_recorder.lock() {
                Ok(tracker) => tracker,
                Err(e) => {
                    activity_recorder_logger.log(format!("Failed to lock token tracker for activity recording: {}", e).red().to_string());
                    continue;
                }
            };
            
            // Get a list of all tracked tokens
            let extended_tokens_arc = tracker.get_extended_tokens_map_arc();
            
            // Process tokens in a separate scope to ensure the mutex guard is dropped
            {
                // Explicitly bind the mutex guard to a variable to control its lifetime
                let tokens_map_guard = match extended_tokens_arc.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        activity_recorder_logger.log(
                            format!("Failed to acquire lock on extended tokens map: {}", e)
                                .red().to_string()
                        );
                        continue;
                    }
                };
                
                // Now we can iterate over the map using the guard
                for (token_mint, token_info) in tokens_map_guard.iter() {
                    // Only save data for tokens that meet the criteria
                    let min_buy_sell_count = std::env::var("MIN_NUMBER_OF_BUY_SELL")
                        .ok()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(50);
                    
                    let max_buy_sell_count = std::env::var("MAX_NUMBER_OF_BUY_SELL")
                        .ok()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(2000);
                    
                    let total_tx_count = token_info.buy_tx_num + token_info.sell_tx_num;
                    
                    // Skip tokens that don't meet the buy/sell count criteria
                    if total_tx_count < min_buy_sell_count || total_tx_count > max_buy_sell_count {
                        continue;
                    }
                    
                    // Get token lifetime in milliseconds
                    let token_lifetime_ms = token_info.token_mint_timestamp.elapsed().as_millis() as u64;
                    
                    // Skip tokens that haven't existed long enough
                    let min_last_time = std::env::var("MIN_LAST_TIME")
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(200000); // Default to 200 seconds (200,000 ms)
                    
                    if token_lifetime_ms < min_last_time {
                        continue;
                    }
                    
                    // Create token activity data
                    let timestamp = chrono::Utc::now().to_rfc3339();
                    let token_name = token_info.token_name.clone().unwrap_or_else(|| "Unknown".to_string());
                    let token_symbol = token_info.token_symbol.clone().unwrap_or_else(|| "Unknown".to_string());
                    
                    let activity_data = serde_json::json!({
                        "timestamp": timestamp,
                        "token_mint": token_mint,
                        "token_name": token_name,
                        "token_symbol": token_symbol,
                        "current_price": token_info.current_token_price,
                        "max_price": token_info.max_token_price,
                        "total_supply": token_info.total_supply,
                        "buy_tx_count": token_info.buy_tx_num,
                        "sell_tx_count": token_info.sell_tx_num,
                        "total_tx_count": total_tx_count,
                        "age_ms": token_lifetime_ms,
                        "market_cap": token_info.market_cap,
                        "volume": token_info.volume,
                        "dev_buy_amount": token_info.dev_buy_amount,
                        "launcher_sol_balance": token_info.launcher_sol_balance,
                        "dev_wallet": token_info.dev_wallet
                    });
                    
                    // Create file path
                    let file_path = format!("{}/{}_activity.json", data_dir, token_mint);
                    
                    // Save the data
                    match serde_json::to_string_pretty(&activity_data) {
                        Ok(json_data) => {
                            if let Err(e) = std::fs::write(&file_path, json_data) {
                                activity_recorder_logger.log(
                                    format!("Failed to save token activity data for {}: {}", token_mint, e)
                                        .red().to_string()
                                );
                            } else {
                                activity_recorder_logger.log(
                                    format!("Saved token activity data for {}", token_mint)
                                        .green().to_string()
                                );
                            }
                        },
                        Err(e) => {
                            activity_recorder_logger.log(
                                format!("Failed to serialize token activity data for {}: {}", token_mint, e)
                                    .red().to_string()
                            );
                        }
                    }
                }
                // MutexGuard is automatically dropped when tokens_map_guard goes out of scope here
            }; // Semicolon added as per the compiler suggestion
        }
    });
    
    // Start sell manager monitoring loop
    let _sell_manager_handle = {
        let sell_manager_clone = Arc::clone(&sell_manager);
        let swapx_clone = swapx.clone();
        let swap_config_clone = swap_config.clone(); // Remove Arc::new() here
        let existing_liquidity_pools_clone = Arc::clone(&existing_liquidity_pools);
        let telegram_service_clone = telegram_service.clone();
        let telegram_chat_id_clone = telegram_chat_id.clone();
        
        tokio::spawn(async move {
            // First, clone everything we need into owned variables 
            let sell_manager = {
                let guard = sell_manager_clone.lock().unwrap();
                guard.clone()
            };
            
            // Create a separate function that never uses MutexGuard across await points
            async fn start_sell_monitoring(
                sell_manager: SellManager,
                swapx: Pump,
                swap_config: Arc<SwapConfig>,
                existing_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
                telegram_service: Option<Arc<TelegramService>>,
                telegram_chat_id: String
            ) {
                // Call the owned SellManager's start_monitoring_loop
                sell_manager.start_monitoring_loop(
                    swapx,
                    (*swap_config).clone(), // Dereference the Arc to get the SwapConfig
                    existing_pools,
                    telegram_service,
                    telegram_chat_id,
                ).await;
            }
            
            // Call the isolated function with cloned/owned values
            start_sell_monitoring(
                sell_manager,
                swapx_clone,
                swap_config_clone,
                existing_liquidity_pools_clone,
                telegram_service_clone,
                telegram_chat_id_clone
            ).await;
        });
    };
    
    // Main connection and processing loop with reconnection logic
    let _retry_count = 0;
    
    loop {
        // Initialize connection
        let logger_clone = logger.clone();
        let (_client, subscribe_tx, mut update_rx) = create_grpc_client(
            &yellowstone_grpc_http, 
            &yellowstone_grpc_token, 
            &filter_config, 
            &logger_clone
        ).await.map_err(|e| e.to_string())?;
        
        // Set up ping interval for keeping the connection alive
        let ping_tx = subscribe_tx.clone();
        let ping_logger = logger.clone();
        
        // Spawn a background task to send periodic pings
        let ping_task = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(yellowstone_ping_interval));
            let mut ping_id: i32 = 1;
            
            loop {
                interval.tick().await;
                
                // Create a ping message
                let ping_request = SubscribeRequest {
                    slots: HashMap::new(),
                    accounts: HashMap::new(),
                    transactions: HashMap::new(),
                    transactions_status: HashMap::new(),
                    entry: HashMap::new(),
                    blocks: HashMap::new(),
                    blocks_meta: HashMap::new(),
                    commitment: None,
                    accounts_data_slice: vec![],
                    ping: Some(SubscribeRequestPing {
                        id: ping_id,
                    }),
                    from_slot: None,
                };
                
                // Send the ping and handle any errors
                if let Err(e) = ping_tx.send(ping_request) {
                    ping_logger.log(format!("[PING ERROR] => Failed to send ping: {}", e).red().to_string());
                    break;
                }
                
                ping_logger.log(format!("[PING] => Sent ping {}", ping_id).blue().to_string());
                ping_id += 1;
            }
        });
        logger.clone().log("[ENHANCED MONITORING STARTED]...".blue().bold().to_string());
        // Process incoming messages (use loop and recv() instead of stream.next())
        loop {
            let logger_clone = logger.clone();
            match update_rx.recv().await {
                Some(msg) => {
                    // Handle ping responses or transaction updates
                    if let Some(update) = msg.update_oneof {
                        match update {
                            UpdateOneof::Ping(_ping) => {
                                logger_clone.log(format!("[PONG] => Received ping response").blue().to_string());
                            },
                            UpdateOneof::Transaction(txn) => {
                                // Process transaction
                                let _start_time = Instant::now();
                                
                                // First, check if transaction exists and has metadata
                                let transaction_meta = txn
                                    .clone()
                                    .transaction
                                    .and_then(|txn1| txn1.meta);
                                
                                if transaction_meta.is_none() {
                                    logger_clone.log("Skipping transaction without metadata".blue().to_string());
                                    continue;
                                }
                                
                                // Get log messages if available
                                if let Some(log_messages) = transaction_meta.map(|meta| meta.log_messages) {
                                    // Important: Check if this transaction involves the PumpFun program before processing
                                    let is_pumpfun_tx = log_messages.iter().any(|log| log.contains(PUMP_PROGRAM));
                                    // Skip if not a PumpFun transaction
                                    if !is_pumpfun_tx {
                                        continue;
                                    }
                                    
                                    // Check for specific PumpFun instructions that we care about
                                    let has_pumpfun_instruction = log_messages.iter().any(|log| 
                                        (log.contains("Instruction: Create") || 
                                         log.contains("Instruction: Buy") || 
                                         log.contains("Instruction: Sell"))
                                    );
                                    
                                    // Also check if the transaction is purely a ComputeBudget transaction
                                    let _is_only_compute_budget = log_messages.iter()
                                        .filter(|log| log.contains("Program") && log.contains("invoke"))
                                        .all(|log| log.contains("ComputeBudget"));
                                    if !has_pumpfun_instruction {continue;}

                                    // Check for program data prefixes that indicate create/buy/sell operations
                                    let is_create_tx = log_messages.iter().any(|log| log.starts_with(MONITOR_PUMPFUN_CREATE_DATA_PREFIX));
                                    let _is_buy_sell_tx = log_messages.iter().any(|log| log.starts_with(PUMP_FUN_BUY_OR_SELL_PROGRAM_DATA_PREFIX));

                                    let mut _mint_flag = false;
                                    
                                    if is_create_tx {
                                        _mint_flag = true;
                                    }

                                    let trade_info = match ParsedTransactionInfo::from_json(&txn) {
                                        Ok(info) => {
                                            info
                                        },
                                        Err(e) => {
                                            for (i, log) in log_messages.iter().enumerate() {
                                                if log.contains("Instruction:") || log.contains("Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P") {
                                                    logger_clone.log(format!("Log[{}]: {}", i, log).yellow().to_string());
                                                }
                                                
                                                // Try to extract token mint from Program data logs for debugging
                                                if log.starts_with("Program data:") {
                                                    logger_clone.log(format!("Program data found at log[{}]", i).yellow().to_string());
                                                    
                                                    // Extract the base64 encoded data
                                                    let parts: Vec<&str> = log.split_whitespace().collect();
                                                    if parts.len() >= 2 {
                                                        let encoded_data = parts[2]; // The actual base64 data
                                                        
                                                        // Try to decode the program data
                                                        if let Ok(decoded_data) = base64::decode(encoded_data) {
                                                            if decoded_data.len() >= 40 {
                                                                let possible_pubkey_data = &decoded_data[8..40]; // Skip 8-byte header, take next 32 bytes
                                                                if let Ok(pubkey) = Pubkey::try_from(possible_pubkey_data) {
                                                                    logger_clone.log(format!("Possible token mint from program data: {}", pubkey).green().to_string());
                                                                }
                                                            }
                                                            
                                                            // Try additional offsets in case token is at a different position
                                                            if decoded_data.len() >= 72 {
                                                                let possible_pubkey_data2 = &decoded_data[40..72]; // Second pubkey
                                                                if let Ok(pubkey) = Pubkey::try_from(possible_pubkey_data2) {
                                                                    logger_clone.log(format!("Second possible pubkey from program data: {}", pubkey).green().to_string());
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            continue;
                                        }
                                    };

                                    // Process token with TokenListManager - check blacklist first
                                    if token_list_manager.is_blacklisted(&trade_info.mint).await {
                                        continue; // Skip blacklisted tokens entirely
                                    }

                                    // Use the transaction type from the parsed transaction info
                                    _mint_flag = matches!(trade_info.transaction_type, TransactionType::Mint);

                                    // Process token transaction
                                    if _mint_flag {
                                        // New token mint detected
                                        let dev_buy_amount = lamports_to_sol(trade_info.volume_change as u64);

                                        // Check dev buy amount
                                        if dev_buy_amount < min_dev_buy as f64 {
                                            logger_clone.log(format!(
                                                "\n\t * [DEV BOUGHT LESS THAN] => {} > {}",
                                                min_dev_buy, dev_buy_amount
                                            ).yellow().to_string());
                                            continue;
                                        } else if dev_buy_amount > max_dev_buy as f64 {
                                            logger_clone.log(format!(
                                                "\n\t * [DEV BOUGHT MORE THAN] => {} < {}",
                                                max_dev_buy, dev_buy_amount
                                            ).yellow().to_string());
                                            continue;
                                        }
                                        
                                        // Get launcher SOL balance
                                        let target_pubkey = match Pubkey::from_str(&trade_info.sender) {
                                            Ok(pk) => pk,
                                            Err(e) => {
                                                logger_clone.log(format!("Invalid sender pubkey: {}", e).red().to_string());
                                                continue;
                                            }
                                        };
                                        
                                        let launcher_sol_balance = match TokenTracker::fetch_launcher_sol_balance(
                                            rpc_nonblocking_client.clone(),
                                            &target_pubkey
                                        ).await {
                                            Ok(balance) => balance,
                                            Err(e) => {
                                                logger_clone.log(format!("Failed to get launcher balance: {}", e).red().to_string());
                                                0.0 // Default to 0 if we can't fetch
                                            }
                                        };
                                        
                                        // Check launcher SOL balance
                                        if launcher_sol_balance < min_launcher_sol {
                                            logger_clone.log(format!(
                                                "\n\t * [LAUNCHER SOL LESS THAN] => {} > {}",
                                                min_launcher_sol, launcher_sol_balance
                                            ).yellow().to_string());
                                            
                                            if !bundle_check {
                                                continue;
                                            }
                                        } else if launcher_sol_balance > max_launcher_sol {
                                            logger_clone.log(format!(
                                                "\n\t * [LAUNCHER SOL MORE THAN] => {} < {}",
                                                max_launcher_sol, launcher_sol_balance
                                            ).yellow().to_string());
                                            
                                            if !bundle_check {
                                                continue;
                                            }
                                        }
                                        
                                        // Calculate the actual token price based on bonding curve reserves
                                        let token_price = if trade_info.bonding_curve_info.new_virtual_token_reserve > 0 {
                                            (trade_info.bonding_curve_info.new_virtual_sol_reserve as f64 / 
                                            trade_info.bonding_curve_info.new_virtual_token_reserve as f64) * 
                                            LAMPORTS_PER_SOL as f64
                                        } else {
                                            0.0 // Fallback to using dev_buy_amount if we can't calculate from reserves
                                        };
                                        
                                        logger_clone.log(format!(
                                            "\n\t * [TOKEN NAME] => {} \n\t * [TOKEN SYMBOL] => {}",
                                            trade_info.token_name.as_ref().unwrap_or(&"Unknown".to_string()),
                                            trade_info.token_symbol.as_ref().unwrap_or(&"Unknown".to_string()),
                                        ).cyan().to_string());
                                        
                                        // Add to token tracker
                                        {
                                            let tracker = token_tracker.lock().unwrap();
                                            if let Err(e) = tracker.add_token(
                                                trade_info.mint.clone(),
                                                token_price, // Use calculated token price instead of dev_buy_amount
                                                launcher_sol_balance,
                                                Some(dev_buy_amount),
                                                Some(launcher_sol_balance),
                                                Some(bundle_check),
                                                trade_info.token_name.clone(),
                                                trade_info.token_symbol.clone(),
                                            ).await {
                                                logger_clone.log(format!("Failed to add token to tracker: {}", e).red().to_string());
                                            }
                                        }
                                        
                                        // If not blacklisted, add to whitelist for monitoring
                                        if token_list_manager.add_to_whitelist(&trade_info.mint).await {
                                            logger_clone.log(format!("Added token {} to whitelist", trade_info.mint).green().to_string());
                                        } else {
                                            logger_clone.log(format!("Token {} already in whitelist", trade_info.mint).yellow().to_string());
                                        }
                                        
                                        // If Telegram service is enabled, check if token passes the filters
                                        if let Some(telegram) = &telegram_service {
                                            // Get filter settings from Telegram service instead of checking environment variable
                                            let filter_settings = telegram.get_filter_settings();
                                            let buy_sell_count_enabled = filter_settings.buy_sell_count_enabled;

                                            // If buy/sell count filter is enabled, we don't send telegram notification yet
                                            // We'll just store the token in cache and monitor it
                                            if buy_sell_count_enabled {
                                                logger_clone.log(format!(
                                                    "Token {} detected with buy/sell count 1 - Will monitor until it reaches threshold",
                                                    trade_info.mint
                                                ).yellow().to_string());
                                                continue;
                                            }

                                            // For other cases, proceed with filter check
                                            let token_info = TokenInfo {
                                                address: trade_info.mint.clone(),
                                                name: trade_info.token_name.clone(),
                                                symbol: trade_info.token_symbol.clone(),
                                                market_cap: Some(dev_buy_amount), // Using dev buy amount as proxy for now
                                                volume: Some(dev_buy_amount),
                                                buy_sell_count: Some(1),
                                                dev_buy_amount: Some(dev_buy_amount),
                                                launcher_sol_balance: Some(launcher_sol_balance),
                                                bundle_check: Some(bundle_check),
                                                ath: Some(token_price), // Initial ATH should be the initial calculated price
                                                buy_ratio: Some(100),      // First transaction is always a buy
                                                sell_ratio: Some(0),       // No sells initially
                                                total_buy_sell: Some(1),    // 1 transaction so far
                                                volume_buy: Some(dev_buy_amount), // All volume is buy volume
                                                volume_sell: Some(0.0),    // No sell volume yet
                                                token_price: Some(token_price), // Now using actual calculated price
                                                dev_wallet: Some(trade_info.sender.clone()),
                                                sol_balance: Some(launcher_sol_balance),
                                            };
                                            
                                            if telegram.token_passes_filters(&token_info) {
                                                if let Err(e) = telegram.send_token_notification(&token_info).await {
                                                    logger_clone.log(format!("Failed to send Telegram notification: {}", e).red().to_string());
                                                }
                                            } else {
                                                // Get current filter settings
                                                let filter_settings = telegram.get_filter_settings();
                                                logger_clone.log(format!(
                                                    "Token {} didn't pass Telegram filters. Current filter configuration:\n\
                                                    Market Cap ({}): {}-{}K\n\
                                                    Volume ({}): {}-{}K\n\
                                                    Buy/Sell Count ({}): {}-{}\n\
                                                    SOL Invested ({}): {} SOL\n\
                                                    Launcher SOL Balance ({}): {}-{} SOL\n\
                                                    Dev Buy & Bundle ({}): {}-{} SOL",
                                                    token_info.address,
                                                    if filter_settings.market_cap_enabled { "✅" } else { "❌" },
                                                    filter_settings.market_cap.min,
                                                    filter_settings.market_cap.max,
                                                    if filter_settings.volume_enabled { "✅" } else { "❌" },
                                                    filter_settings.volume.min,
                                                    filter_settings.volume.max,
                                                    if filter_settings.buy_sell_count_enabled { "✅" } else { "❌" },
                                                    filter_settings.buy_sell_count.min,
                                                    filter_settings.buy_sell_count.max,
                                                    if filter_settings.sol_invested_enabled { "✅" } else { "❌" },
                                                    filter_settings.sol_invested,
                                                    if filter_settings.launcher_sol_balance_enabled { "✅" } else { "❌" },
                                                    filter_settings.launcher_sol_balance.min,
                                                    filter_settings.launcher_sol_balance.max,
                                                    if filter_settings.dev_buy_bundle_enabled { "✅" } else { "❌" },
                                                    filter_settings.dev_buy_bundle.min,
                                                    filter_settings.dev_buy_bundle.max
                                                ).yellow().to_string());
                                                continue;
                                            }
                                        }

                                        // Check if we should run the buy processor
                                        let should_run_buy_processor = {
                                            let tracker = token_tracker.lock().unwrap();
                                            
                                            // Use the is_token_monitored method
                                            tracker.is_token_monitored(&trade_info.mint)
                                        };

                                        // Only spawn a buy task if the token meets all criteria
                                        if !should_run_buy_processor {
                                            logger_clone.log(format!(
                                                "Token {} not ready for buy processor yet - criteria not met",
                                                trade_info.mint
                                            ).yellow().to_string());
                                            continue;
                                        }

                                        // Spawn a buy task for this token since it meets all criteria
                                        let swapx_clone = swapx.clone();
                                        let swap_config_clone = swap_config.clone();
                                        let token_tracker_clone = Arc::clone(&token_tracker);
                                        let _sell_manager_clone: Arc<Mutex<SellManager>> = Arc::clone(&sell_manager);
                                        let telegram_service_clone = telegram_service.clone();
                                        let token_mint_clone = trade_info.mint.to_string();
                                        let existing_liquidity_pools_clone = Arc::clone(&existing_liquidity_pools);
                                        let telegram_chat_id_clone = telegram_chat_id.clone();
                                        let _buy_manager_clone = buy_manager.clone();
                                        let _logger_for_task = logger.clone();
                                        
                                        // Get current token information
                                        let token_data = {
                                            let tracker = token_tracker_clone.lock().unwrap();
                                            let extended_tokens_map = tracker.get_extended_tokens_map_arc();
                                            let extended_tokens = extended_tokens_map.lock().unwrap();
                                            extended_tokens.get(&trade_info.mint).map(|t| t.clone_without_task())
                                        };
                                            
                                        if let Some(token_data) = token_data {
                                            // Get current price for buy task
                                            let _current_price = token_data.current_token_price;
                                            
                                            // Create buy_manager_clone before using it
                                            let buy_manager_clone = buy_manager.clone();
                                            
                                            // Convert TokenInfo to ExtendedTokenInfo with the needed fields
                                            let extended_info = ExtendedTokenInfo {
                                                token_mint: token_data.token_mint.clone(),
                                                token_name: token_data.token_name.clone(),
                                                token_symbol: token_data.token_symbol.clone(),
                                                current_token_price: token_data.current_token_price,
                                                max_token_price: token_data.max_token_price,
                                                initial_price: token_data.current_token_price, // Use current price as initial price
                                                total_supply: token_data.total_supply,
                                                buy_tx_num: token_data.buy_tx_num,
                                                sell_tx_num: token_data.sell_tx_num,
                                                token_mint_timestamp: token_data.token_mint_timestamp,
                                                launcher_sol_balance: token_data.launcher_sol_balance,
                                                market_cap: token_data.market_cap,
                                                volume: token_data.volume,
                                                dev_buy_amount: token_data.dev_buy_amount,
                                                bundle_check: token_data.bundle_check,
                                                active_task: None,
                                                is_monitored: true,
                                                dev_wallet: token_data.dev_wallet.clone(),
                                                token_amount: None, // Initialize token amount as None
                                            };
                                            
                                            // Replace the tokio::spawn in the token handling section 
                                            tokio::spawn(async move {
                                                // First, get a fully owned BuyManager
                                                let buy_manager = {
                                                    let guard = buy_manager_clone.lock().unwrap();
                                                    guard.clone()
                                                };
                                                
                                                // Create a separate function that never uses MutexGuard across await points
                                                async fn execute_buy_task(
                                                    mut buy_manager: BuyManager,
                                                    token_mint: String,
                                                    swapx: Pump,
                                                    swap_config: Arc<SwapConfig>,
                                                    existing_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
                                                    telegram_service: Option<Arc<TelegramService>>,
                                                    telegram_chat_id: String,
                                                    extended_info: ExtendedTokenInfo
                                                ) {
                                                  let _ =  buy_manager.execute_buy(
                                                        &token_mint,
                                                        &swapx,
                                                        &(*swap_config), // Dereference the Arc
                                                        &existing_pools,
                                                        &telegram_service,
                                                        &telegram_chat_id,
                                                        &extended_info,
                                                        None,
                                                    ).await;
                                                }
                                                
                                                // Call the isolated function with owned values
                                                execute_buy_task(
                                                    buy_manager,
                                                    token_mint_clone,
                                                    swapx_clone,
                                                    swap_config_clone,
                                                    existing_liquidity_pools_clone,
                                                    telegram_service_clone,
                                                    telegram_chat_id_clone,
                                                    extended_info
                                                ).await;
                                            });
                                        }
                                    } else {
                                        let token_tracker = token_tracker.lock().unwrap();
                                        // Replace iter call with direct access through extended_tokens map
                                        let extended_tokens_map = token_tracker.get_extended_tokens_map_arc();
                                        let mut extended_tokens = extended_tokens_map.lock().unwrap();
                                        let mut current_token_in_token_tracker = extended_tokens.get_mut(&trade_info.mint);
                                        
                                        match trade_info.transaction_type {
                                            TransactionType::Buy => {
                                                // Update buy count and volume
                                                if let Some(ref mut token) = current_token_in_token_tracker {
                                                    token.buy_tx_num += 1;
                                                    token.volume = Some(token.volume.unwrap_or(0.0) + trade_info.volume_change.abs() as f64 / LAMPORTS_PER_SOL as f64);
                                                    
                                                    // Update market cap if available
                                                    if let Some(market_cap) = Some(trade_info.market_cap) {
                                                        token.market_cap = Some(market_cap);
                                                    }
                                                    
                                                    // Update current price
                                                    token.current_token_price = trade_info.price_per_token;
                                                    
                                                    // Update max price if current price is higher
                                                    if trade_info.price_per_token > token.max_token_price {
                                                        token.max_token_price = trade_info.price_per_token;
                                                    }
                                                    
                                                    // Log the buy transaction
                                                    logger_clone.log(format!(
                                                        "Token {} buy transaction processed - Buy count: {}, Volume: {} SOL",
                                                        trade_info.mint,
                                                        token.buy_tx_num,
                                                        token.volume.unwrap_or(0.0)
                                                    ).green().to_string());
                                                }
                                            },
                                            TransactionType::Sell => {
                                                // Update sell count and volume
                                                if let Some(ref mut token) = current_token_in_token_tracker {
                                                    token.sell_tx_num += 1;
                                                    token.volume = Some(token.volume.unwrap_or(0.0) + trade_info.volume_change.abs() as f64 / LAMPORTS_PER_SOL as f64);
                                                    
                                                    // Update current price
                                                    token.current_token_price = trade_info.price_per_token;
                                                    
                                                    // Log the sell transaction
                                                    logger_clone.log(format!(
                                                        "Token {} sell transaction processed - Sell count: {}, Volume: {} SOL",
                                                        trade_info.mint,
                                                        token.sell_tx_num,
                                                        token.volume.unwrap_or(0.0)
                                                    ).yellow().to_string());
                                                }
                                            },
                                            TransactionType::Mint => {
                                                // Mint transactions are handled separately in the mint_flag section
                                                logger_clone.log(format!(
                                                    "Skipping mint transaction for {} as it's handled in mint section",
                                                    trade_info.mint
                                                ).blue().to_string());
                                            }
                                        }

                                        // Check if token meets criteria for buy after transaction
                                        let token_age = SystemTime::now()
                                            .duration_since(SystemTime::now() - Duration::from_millis(token_tracker.get_token_lifetime_ms(&trade_info.mint).unwrap_or(0)))
                                            .unwrap_or(Duration::from_secs(0))
                                            .as_millis() as u64;

                                        // If token is still within the wait time window, update its parameters
                                        if token_age < limit_wait_time {
                                            if let Some(ref mut token) = current_token_in_token_tracker {
                                                // Update token parameters
                                                token.dev_buy_amount = Some(trade_info.volume_change.abs() as f64 / LAMPORTS_PER_SOL as f64);
                                                token.launcher_sol_balance = Some(trade_info.launcher_sol_balance);
                                                token.bundle_check = Some(bundle_check);
                                                
                                                logger_clone.log(format!(
                                                    "Updated token {} parameters within wait time window ({} ms remaining)",
                                                    trade_info.mint,
                                                    limit_wait_time - token_age
                                                ).cyan().to_string());
                                            }
                                        }

                                        // Check if token meets all criteria for buy
                                        if let Some(ref mut token) = current_token_in_token_tracker {
                                            let meets_criteria = token.buy_tx_num >= min_buy_sell_count &&
                                                token.buy_tx_num <= max_buy_sell_count &&
                                                token.volume.unwrap_or(0.0) >= min_volume &&
                                                token.volume.unwrap_or(0.0) <= max_volume &&
                                                token.market_cap.unwrap_or(0.0) >= min_market_cap &&
                                                token.market_cap.unwrap_or(0.0) <= max_market_cap;

                                            if meets_criteria {
                                                logger_clone.log(format!(
                                                    "Token {} meets all criteria for buy - Executing buy strategy",
                                                    trade_info.mint
                                                ).green().to_string());

                                                // Execute buy strategy
                                                let buy_manager_clone = buy_manager.clone();
                                                let swapx_clone = swapx.clone();
                                                let swap_config_clone = swap_config.clone();
                                                let existing_liquidity_pools_clone = Arc::clone(&existing_liquidity_pools);
                                                let telegram_service_clone = telegram_service.clone();
                                                let telegram_chat_id_clone = telegram_chat_id.clone();
                                                let token_mint_clone = trade_info.mint.clone();
                                                let token_info = token.clone_without_task();
                                                let _logger_for_spawn = logger.clone();

                                                tokio::spawn(async move {
                                                    let mut buy_manager = {
                                                        let guard = buy_manager_clone.lock().unwrap();
                                                        guard.clone()
                                                    };

                                                    if let Err(e) = buy_manager.execute_buy(
                                                        &token_mint_clone,
                                                        &swapx_clone,
                                                        &(*swap_config_clone),
                                                        &existing_liquidity_pools_clone,
                                                        &telegram_service_clone,
                                                        &telegram_chat_id_clone,
                                                        &token_info,
                                                        None,
                                                    ).await {
                                                        _logger_for_spawn.log(format!(
                                                            "Failed to execute buy for token {}: {}",
                                                            token_mint_clone, e
                                                        ).red().to_string());
                                                    }
                                                });
                                            } else {
                                                logger_clone.log(format!(
                                                    "Token {} does not meet all criteria for buy - Continuing to monitor",
                                                    trade_info.mint
                                                ).yellow().to_string());
                                            }
                                        }
                                    }
                                }
                            },
                            _ => {} // Other update types
                        }
                    }
                },
                None => {
                    let logger_clone = logger.clone();
                    logger_clone.log(
                        format!("Yellowstone gRPC stream ended. Attempting to reconnect...")
                            .red()
                            .to_string(),
                    );
                    // Cancel the ping task before reconnecting
                    ping_task.abort();
                    // Sleep before attempting to reconnect
                    time::sleep(Duration::from_secs(yellowstone_reconnect_delay)).await;
                    // Break the inner message loop to reconnect
                    break;
                }
            }
        }
        // If we get here, the stream has ended or errored, so we'll reconnect
        let logger_clone = logger.clone();
        logger_clone.log("Yellowstone gRPC stream ended. Reconnecting...".yellow().to_string());
        // If ping task is still running, abort it
        ping_task.abort();
        // Sleep before reconnecting
        time::sleep(Duration::from_secs(yellowstone_reconnect_delay)).await;
    }
}

#[allow(dead_code)]
pub struct EnhancedMonitor {
    config: Config,
    client: GeyserGrpcClient<InterceptorXToken>,
    token_tracker: Arc<TokenTracker>,
    sell_manager: Arc<Mutex<SellManager>>,
    buy_manager: Arc<Mutex<BuyManager>>,
    swapx: Pump,
    existing_liquidity_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
    telegram_service: Option<Arc<TelegramService>>,
    telegram_chat_id: String,
    logger: Logger,
    token_list_manager: Arc<TokenListManager>, // Add this field
}

impl EnhancedMonitor {
    pub async fn new(
        config: Config,
        token_tracker: Arc<TokenTracker>,
        sell_manager: Arc<Mutex<SellManager>>,
        swapx: Pump,
        existing_liquidity_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
        telegram_service: Option<Arc<TelegramService>>,
        telegram_chat_id: String,
        logger: Logger,
    ) -> Result<Self> {
        // Initialize gRPC client for Yellowstone subscription
        let filter_config = FilterConfig {
            program_ids: vec![
                PUMP_PROGRAM.to_string(),
                // Add other program IDs if needed
            ],
            _instruction_discriminators: vec![PUMP_FUN_CREATE_IX_DISCRIMINATOR],
        };
        
        // Create a new buy manager
        let buy_manager = Arc::new(Mutex::new(BuyManager::new(logger.clone())));
        
        // Create token list manager
        let token_list_manager = match TokenListManager::new(
            "./whitelist.json", 
            "./blacklist.json", 
            120000, // Default review cycle: 2 minutes
            600000, // Default save interval: 10 minutes
            logger.clone()
        ) {
            Ok(manager) => Arc::new(manager),
            Err(e) => return Err(anyhow!("Failed to initialize token list manager: {}", e)),
        };
        
        let (client, _, _) = create_grpc_client(
            &config.yellowstone_grpc_http,
            &config.yellowstone_grpc_token,
            &filter_config,
            &logger,
        ).await.map_err(|e| anyhow!("Failed to connect to Yellowstone: {}", e))?;
        
        Ok(Self {
            config,
            client,
            token_tracker,
            sell_manager,
            buy_manager,
            swapx,
            existing_liquidity_pools,
            telegram_service,
            telegram_chat_id,
            logger,
            token_list_manager,
        })
    }

    // Helper to extract token mint from logs
    fn extract_token_mint_from_logs(&self, log_messages: &[String]) -> Option<String> {
        for log in log_messages {
            // Look for log line with token mint address pattern
            if log.contains("Token Mint:") {
                // Extract the token mint address
                let parts: Vec<&str> = log.split("Token Mint:").collect();
                if parts.len() > 1 {
                    let mint_parts: Vec<&str> = parts[1].trim().split_whitespace().collect();
                    if !mint_parts.is_empty() {
                        return Some(mint_parts[0].to_string());
                    }
                }
            }
            
            // Another approach - look for SPL token program mentions
            if log.contains("spl-token") && log.contains("create-token") {
                let parts: Vec<&str> = log.split_whitespace().collect();
                for (i, part) in parts.iter().enumerate() {
                    if *part == "create-token" && i + 1 < parts.len() {
                        return Some(parts[i + 1].to_string());
                    }
                }
            }
            
            // Look for PumpFun create data
            if log.starts_with(MONITOR_PUMPFUN_CREATE_DATA_PREFIX) {
                // Extract and decode the base64 data
                let encoded_data = log.split_whitespace().nth(2).unwrap_or("").to_string();
                
                if !encoded_data.is_empty() {
                    if let Ok(decoded_data) = base64::decode(&encoded_data) {
                        if decoded_data.len() >= 72 { // 8 (discriminator) + name + symbol + uri + mint (32)
                            // Skip name, symbol, uri (variable length)
                            // Extract mint pubkey which is after the variable length fields
                            let mut offset = 8; // Skip discriminator
                            
                            // Skip name
                            if offset + 4 <= decoded_data.len() {
                                let name_length = u32::from_le_bytes(
                                    decoded_data[offset..offset+4].try_into().unwrap_or([0, 0, 0, 0])
                                ) as usize;
                                offset += 4 + name_length;
                            }
                            
                            // Skip symbol
                            if offset + 4 <= decoded_data.len() {
                                let symbol_length = u32::from_le_bytes(
                                    decoded_data[offset..offset+4].try_into().unwrap_or([0, 0, 0, 0])
                                ) as usize;
                                offset += 4 + symbol_length;
                            }
                            
                            // Skip uri
                            if offset + 4 <= decoded_data.len() {
                                let uri_length = u32::from_le_bytes(
                                    decoded_data[offset..offset+4].try_into().unwrap_or([0, 0, 0, 0])
                                ) as usize;
                                offset += 4 + uri_length;
                            }
                            
                            // Extract mint pubkey
                            if offset + 32 <= decoded_data.len() {
                                let key_bytes: [u8; 32] = decoded_data[offset..offset+32]
                                    .try_into()
                                    .unwrap_or([0; 32]);
                                let pubkey = Pubkey::new_from_array(key_bytes);
                                return Some(pubkey.to_string());
                            }
                        }
                    }
                }
            }
            
            // Look for token mint in post token balances
            if log.contains("post_token_balances") && log.contains("mint:") {
                let parts: Vec<&str> = log.split("mint:").collect();
                if parts.len() > 1 {
                    let mint_part = parts[1].trim().split(',').next().unwrap_or("").trim().trim_matches('"');
                    if !mint_part.is_empty() && mint_part.ends_with("pump") {
                        return Some(mint_part.to_string());
                    }
                }
            }
        }
        
        None
    }

    // Start monitoring for tokens based on the enhanced criteria
    pub async fn start(&self) -> Result<()> {
        self.logger.log("Starting enhanced monitor...".to_string());
        
        // Start the token tracker
        self.token_tracker.start();
        
        // Create a clone of the buy manager for the background task
        let buy_manager_clone = self.buy_manager.clone();
        let token_tracker_clone = self.token_tracker.clone();
        let swapx_clone = self.swapx.clone();
        let swap_config_clone = Arc::new(self.config.swap_config.clone());
        let existing_pools_clone = self.existing_liquidity_pools.clone();
        let telegram_service_clone = self.telegram_service.clone();
        let telegram_chat_id_clone = self.telegram_chat_id.clone();
        
        // Start the buy manager in a background task, using a completely different approach
        // that avoids any potential Send trait issues
        tokio::spawn(async move {
            // First, clone everything we need into owned variables
            let buy_manager = {
                let guard = buy_manager_clone.lock().unwrap();
                guard.clone()
            };
            
            // Create a separate function that never uses MutexGuard across await points
            async fn start_buy_monitoring(
                buy_manager: BuyManager,
                token_tracker: Arc<TokenTracker>, 
                swapx: Pump,
                swap_config: Arc<SwapConfig>,
                existing_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
                telegram_service: Option<Arc<TelegramService>>,
                telegram_chat_id: String
            ) {
                let monitoring_interval = Duration::from_secs(2);
                let mut interval = time::interval(monitoring_interval);
                let mut manager = buy_manager; // Use local mutable manager
                
                loop {
                    interval.tick().await;
                    
                    // Check and reset daily budget if needed
                    manager.check_and_reset_daily_budget();
                    
                    // Get current tracked tokens - IMPORTANT: Create a separate scope for the lock
                    // to ensure it's dropped before any await points
                    let token_data: Vec<(String, ExtendedTokenInfo)> = {
                        let extended_tokens_map = token_tracker.get_extended_tokens_map_arc();
                        let extended_tokens = extended_tokens_map.lock().unwrap();
                        
                        // Clone the data we need to process outside the lock
                        extended_tokens.iter()
                            .map(|(token_mint, token_info)| (token_mint.clone(), token_info.clone_without_task()))
                            .collect()
                    };
                    
                    // Process tokens for potential buys
                    for (token_mint, token_info) in token_data {
                        // Skip already bought tokens
                        if manager.is_token_bought(&token_mint) {
                            continue;
                        }
                        
                        // Add to strategies if not already present - use check first
                        let strategy_exists = manager.is_token_bought(&token_mint);
                        if !strategy_exists {
                            let _ = manager.add_token_strategy(token_mint.clone(), token_info.current_token_price);
                        }
                        
                        // Update price in strategy
                        let _ = manager.update_token_price(&token_mint, token_info.current_token_price);
                        
                        // Try to execute buy
                        // Note: No bonding curve info provided here, the function will fetch it if needed
                        if let Err(e) = manager.execute_buy(
                            &token_mint,
                            &swapx,
                            &swap_config,
                            &existing_pools,
                            &telegram_service,
                            &telegram_chat_id,
                            &token_info,
                            None,
                        ).await {
                            // Use a local log since we don't have direct access to logger
                            eprintln!("Error executing buy for {}: {}", token_mint, e);
                        }
                    }
                    
                    // Clean up old strategies - we can't access private fields directly
                    // So we'll skip this part as BuyManager should handle strategy cleanup internally
                    // or we'll need to add a public method like clean_old_strategies() to BuyManager
                }
            }
            
            // Call the isolated function with cloned/owned values
            start_buy_monitoring(
                buy_manager,
                token_tracker_clone,
                swapx_clone,
                swap_config_clone,
                existing_pools_clone,
                telegram_service_clone,
                telegram_chat_id_clone
            ).await;
        });
        
        // Start the sell manager in a background task
        let sell_manager_clone = self.sell_manager.clone();
        let swapx_clone = self.swapx.clone();
        let swap_config_clone = Arc::new(self.config.swap_config.clone());
        let existing_pools_clone = self.existing_liquidity_pools.clone();
        let telegram_service_clone = self.telegram_service.clone();
        let telegram_chat_id_clone = self.telegram_chat_id.clone();
        
        tokio::spawn(async move {
            // First, clone everything we need into owned variables 
            let sell_manager = {
                let guard = sell_manager_clone.lock().unwrap();
                guard.clone()
            };
            
            // Create a separate function that never uses MutexGuard across await points
            async fn start_sell_monitoring(
                sell_manager: SellManager,
                swapx: Pump,
                swap_config: Arc<SwapConfig>,
                existing_pools: Arc<Mutex<HashSet<LiquidityPool>>>,
                telegram_service: Option<Arc<TelegramService>>,
                telegram_chat_id: String
            ) {
                // Call the owned SellManager's start_monitoring_loop
                sell_manager.start_monitoring_loop(
                    swapx,
                    (*swap_config).clone(), // Dereference the Arc to get the SwapConfig
                    existing_pools,
                    telegram_service,
                    telegram_chat_id,
                ).await;
            }
            
            // Call the isolated function with cloned/owned values
            start_sell_monitoring(
                sell_manager,
                swapx_clone,
                swap_config_clone,
                existing_pools_clone,
                telegram_service_clone,
                telegram_chat_id_clone
            ).await;
        });
        
        // Set up Yellowstone gRPC subscription for real-time token monitoring
        let filter_config = FilterConfig {
            program_ids: vec![PUMP_PROGRAM.to_string()],
            _instruction_discriminators: vec![PUMP_FUN_CREATE_IX_DISCRIMINATOR],
        };
        
        // Set up stream for token transactions
        let (_client, subscribe_tx, mut update_rx) = create_grpc_client(
            &self.config.yellowstone_grpc_http,
            &self.config.yellowstone_grpc_token,
            &filter_config,
            &self.logger,
        ).await.map_err(|e| anyhow!("Failed to connect to Yellowstone: {}", e))?;
        
        // Send ping requests to keep the connection alive
        let ping_interval = Duration::from_secs(self.config.yellowstone_ping_interval);
        let subscribe_tx_clone = subscribe_tx.clone();
        
        tokio::spawn(async move {
            let mut interval = time::interval(ping_interval);
            let mut ping_id: i32 = 1;
            
            loop {
                interval.tick().await;
                let ping_request = SubscribeRequest {
                    slots: HashMap::new(),
                    accounts: HashMap::new(),
                    transactions: HashMap::new(),
                    transactions_status: HashMap::new(),
                    entry: HashMap::new(),
                    blocks: HashMap::new(),
                    blocks_meta: HashMap::new(),
                    commitment: None,
                    accounts_data_slice: vec![],
                    ping: Some(SubscribeRequestPing { id: ping_id }),
                    from_slot: None,
                };
                
                if let Err(e) = subscribe_tx_clone.send(ping_request) {
                    eprintln!("Failed to send ping: {}", e);
                    break;
                }
                
                ping_id += 1;
            }
        });
        
        // Process incoming updates from the stream
        self.logger.log("Starting to process Yellowstone stream updates...".to_string());
        
        // Main processing loop for incoming stream updates
        while let Some(update) = update_rx.recv().await {
            if let Some(update) = update.update_oneof {
                match update {
                    UpdateOneof::Transaction(tx_update) => {
                        // Process transaction update
                        if let Err(e) = self.process_transaction_update(&tx_update).await {
                            self.logger.log(format!("Error processing transaction: {}", e).red().to_string());
                        }
                    },
                    UpdateOneof::Ping(_ping) => {
                        // Just log ping responses at a debug level or ignore
                        // self.logger.log(format!("Received ping: {}", ping_update.id).to_string());
                    },
                    _ => {
                        // Ignore other update types
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Set a custom take profit percentage for a specific token
    pub async fn set_token_take_profit(&self, token_mint: &str, take_profit_pct: f64) -> Result<()> {
        // Find the token in the sell manager's strategies
        let mut sell_manager = self.sell_manager.lock().unwrap();
        
        if let Some(strategy) = sell_manager.get_strategy_mut(token_mint) {
            self.logger.log(format!(
                "Setting custom take profit for {}: {}% (was {}%)",
                token_mint,
                take_profit_pct,
                strategy.take_profit_pct
            ).yellow().to_string());
            
            strategy.take_profit_pct = take_profit_pct;
            Ok(())
        } else {
            Err(anyhow!("Token not found in sell manager: {}", token_mint))
        }
    }
    
    /// Set a custom stop loss percentage for a specific token
    pub async fn set_token_stop_loss(&self, token_mint: &str, stop_loss_pct: f64) -> Result<()> {
        // Find the token in the sell manager's strategies
        let mut sell_manager = self.sell_manager.lock().unwrap();
        
        if let Some(strategy) = sell_manager.get_strategy_mut(token_mint) {
            self.logger.log(format!(
                "Setting custom stop loss for {}: {}% (was {}%)",
                token_mint,
                stop_loss_pct,
                strategy.stop_loss_pct
            ).yellow().to_string());
            
            strategy.stop_loss_pct = stop_loss_pct;
            Ok(())
        } else {
            Err(anyhow!("Token not found in sell manager: {}", token_mint))
        }
    }

    // Fix for token_tracker direct access in manual_buy_token
    pub async fn manual_buy_token(&self, token_mint: &str, amount: f64) -> Result<()> {
        self.logger.log(format!("Manual buy requested for {} with {} SOL", token_mint, amount).blue().to_string());
        
        // Get token info from tracker - correctly access the nested mutex
        let token_info = {
            // TokenTracker is not wrapped in a Mutex, but has internal mutexes
            let extended_tokens_map = self.token_tracker.get_extended_tokens_map_arc();
            let extended_tokens = extended_tokens_map.lock().unwrap();
            
            match extended_tokens.get(token_mint) {
                Some(info) => info.clone_without_task(),
                None => return Err(anyhow!("Token not found in tracker")),
            }
        };
        
        // Setup the buy strategy with the BuyManager
        {
            let mut buy_manager = self.buy_manager.lock().unwrap();
            
            // Remove any existing strategy for this token
            buy_manager.remove_token_strategy(token_mint);
            
            // Add a new strategy with the specified amount
            buy_manager.add_token_strategy(token_mint.to_string(), token_info.current_token_price)?;
            
            // Update the buy amount for this specific token using the new method
            buy_manager.update_token_buy_amount(token_mint, amount)?;
        }
        
        // Get a full clone of the BuyManager for use in the async context
        let buy_manager_clone = {
            let guard = self.buy_manager.lock().unwrap();
            guard.clone()
        };
        
        // Wrap the swap_config in an Arc
        let swap_config_arc = Arc::new(self.config.swap_config.clone());
        
        // Define an isolated function that won't have any MutexGuard crossing await points
        async fn execute_manual_buy(
            mut buy_manager: BuyManager,
            token_mint: String,
            swapx: &Pump,
            swap_config: &Arc<SwapConfig>,
            existing_pools: &Arc<Mutex<HashSet<LiquidityPool>>>,
            telegram_service: &Option<Arc<TelegramService>>,
            telegram_chat_id: &str,
            token_info: ExtendedTokenInfo,
        ) -> Result<bool> {
            buy_manager.execute_buy(
                &token_mint,
                swapx,
                &(**swap_config), // Double dereference to get SwapConfig from &Arc<SwapConfig>
                existing_pools,
                telegram_service,
                telegram_chat_id,
                &token_info,
                None,
            ).await
        }
        
        // Call the isolated function with all needed arguments
        match execute_manual_buy(
            buy_manager_clone,
            token_mint.to_string(),
            &self.swapx,
            &swap_config_arc,
            &self.existing_liquidity_pools,
            &self.telegram_service,
            &self.telegram_chat_id,
            token_info,
        ).await {
            Ok(true) => {
                self.logger.log(format!("Successfully executed manual buy for {}", token_mint).green().to_string());
                Ok(())
            },
            Ok(false) => {
                Err(anyhow!("Buy was not executed - check logs for details"))
            },
            Err(e) => {
                Err(anyhow!("Error executing buy: {}", e))
            }
        }
    }
    
    // Add method to enable/disable auto-buying
    pub fn set_auto_buying_enabled(&self, enabled: bool) -> Result<()> {
        let mut buy_manager = self.buy_manager.lock().unwrap();
        buy_manager.set_active(enabled);
        Ok(())
    }

    // Move process_transaction_update from standalone function to method
    async fn process_transaction_update(&self, tx_update: &SubscribeUpdateTransaction) -> Result<()> {
        // Check if transaction is available
        if let Some(txn) = &tx_update.transaction {
            // Check if metadata is available
            if let Some(tx_meta) = &txn.meta {
                // Check if any log message contains the PumpFun program ID - Do this FIRST
                let is_pumpfun_tx = tx_meta.log_messages.iter().any(|log| log.contains(PUMP_PROGRAM));
                
                if !is_pumpfun_tx {
                    // Skip non-PumpFun transactions with detailed logging
                    if !tx_meta.log_messages.is_empty() {
                        self.logger.log(format!(
                            "Skipping non-PumpFun transaction in process_transaction_update. First 2 logs: {:?}", 
                            tx_meta.log_messages.iter().take(2).collect::<Vec<_>>()
                        ).blue().to_string());
                    }
                    return Ok(());
                }
                
                // Log that we're processing a PumpFun transaction
                self.logger.log(format!(
                    "Processing PumpFun transaction update - TX contains PumpFun program"
                ).cyan().to_string());
                
                match ParsedTransactionInfo::from_json(tx_update) {
                    Ok(parsed_tx) => {
                        // Process based on transaction type
                        match parsed_tx.transaction_type {
                            TransactionType::Mint => {
                                // Handle token creation
                                if let Err(e) = self.handle_token_creation(tx_update, &parsed_tx.mint, &parsed_tx).await {
                                    self.logger.log(format!("Error handling token creation: {}", e).red().to_string());
                                }
                            },
                            TransactionType::Buy | TransactionType::Sell => {
                                // Handle buy/sell transaction
                                if let Err(e) = self.handle_token_transaction(tx_update, &parsed_tx.mint, &parsed_tx).await {
                                    self.logger.log(format!("Error handling token transaction: {}", e).red().to_string());
                                }
                            }
                        }
                    },
                    Err(e) => {
                        self.logger.log(format!("Failed to parse transaction: {}", e).red().to_string());
                        
                        // Check for specific PumpFun data patterns in logs
                        let log_messages = &tx_meta.log_messages;
                        let is_create_tx = log_messages.iter().any(|log| log.starts_with(MONITOR_PUMPFUN_CREATE_DATA_PREFIX));
                        let is_buy_sell_tx = log_messages.iter().any(|log| log.starts_with(PUMP_FUN_BUY_OR_SELL_PROGRAM_DATA_PREFIX));
                        
                        if is_create_tx {
                            // Try to extract token mint from logs for create transaction
                            if let Some(token_mint) = self.extract_token_mint_from_logs(log_messages) {
                                if let Err(e) = self.handle_token_creation(tx_update, &token_mint, &ParsedTransactionInfo::default()).await {
                                    self.logger.log(format!("Error handling token creation (fallback): {}", e).red().to_string());
                                }
                            }
                        } else if is_buy_sell_tx {
                            // Try to extract token mint from logs for buy/sell transaction
                            if let Some(token_mint) = self.extract_token_mint_from_logs(log_messages) {
                                if let Err(e) = self.handle_token_transaction(tx_update, &token_mint, &ParsedTransactionInfo::default()).await {
                                    self.logger.log(format!("Error handling token transaction (fallback): {}", e).red().to_string());
                                }
                            }
                        }
                    }
                }
            } else {
                self.logger.log("Transaction has no metadata".yellow().to_string());
            }
        } else {
            self.logger.log("Received update with no transaction data".yellow().to_string());
        }
        
        Ok(())
    }

    // Fix the handle_token_creation method to properly handle launcher extraction and logging
    async fn handle_token_creation(
        &self, 
        tx_update: &SubscribeUpdateTransaction,
        token_mint: &str,
        parsed_tx: &ParsedTransactionInfo
    ) -> Result<()> {
        self.logger.log(format!("Detected new token creation: {}", token_mint).yellow().to_string());
        
        // Use the parsed transaction data if available
        let _dev_buy_amount = lamports_to_sol(parsed_tx.volume_change.abs() as u64);
        let launcher = if !parsed_tx.sender.is_empty() {
            Pubkey::from_str(&parsed_tx.sender).map_err(|e| anyhow!("Invalid sender pubkey: {}", e))?
        } else {
            // Fallback to extracting from transaction if parsed_tx doesn't have sender
            if let Some(txn) = &tx_update.transaction {
                if let Some(_meta) = &txn.meta {
                    if let Some(message) = txn.transaction.as_ref().and_then(|tx| tx.message.as_ref()) {
                        if !message.account_keys.is_empty() {
                            Pubkey::try_from(&message.account_keys[0][..]).map_err(|e| anyhow!("Invalid account key: {}", e))?
                        } else {
                            return Err(anyhow!("No account keys found"));
                        }
                    } else {
                        return Err(anyhow!("No transaction message found"));
                    }
                } else {
                    return Err(anyhow!("No transaction metadata found"));
                }
            } else {
                return Err(anyhow!("Couldn't determine launcher for token"));
            }
        };
        
        // Get launcher SOL balance
        let launcher_sol = match self.config.app_state.rpc_client.get_balance(&launcher) {
            Ok(balance) => balance as f64 / LAMPORTS_PER_SOL as f64,
            Err(_) => 0.0,
        };
        
        // Get token metadata from parsed transaction
        let token_name = parsed_tx.token_name.clone().unwrap_or_else(|| "Unknown".to_string());
        let token_symbol = parsed_tx.token_symbol.clone().unwrap_or_else(|| "UNKNOWN".to_string());
        
        // Get pool info using the bonding curve info from parsed transaction
        let token_pubkey = Pubkey::from_str(token_mint).map_err(|e| anyhow!("Invalid token mint: {}", e))?;
        
        let pool_info = match self.swapx.get_token_pool_info(&token_pubkey).await {
            Ok(info) => info,
            Err(e) => {
                self.logger.log(format!("Failed to get pool info for {}: {}", token_mint, e).red().to_string());
                return Ok(());
            }
        };
        
        // Calculate token price from pool info or bonding curve info
        let price = if parsed_tx.bonding_curve_info.new_virtual_token_reserve > 0 {
            (parsed_tx.bonding_curve_info.new_virtual_sol_reserve as f64 / 
             parsed_tx.bonding_curve_info.new_virtual_token_reserve as f64) * 
             LAMPORTS_PER_SOL as f64
        } else if pool_info.virtual_token_reserves > 0.0 {
            (pool_info.virtual_sol_reserves as f64 / pool_info.virtual_token_reserves as f64) * 
            LAMPORTS_PER_SOL as f64
        } else {
            // Fallback price calculation
            launcher_sol * 0.1
        };

        // Update token info in the tracker
        if let Err(e) = self.token_tracker.update_token(token_mint, price, true).await {
            self.logger.log(format!("Failed to update token info: {}", e).red().to_string());
            return Ok(());
        }
        
        // Send notification if telegram service is available
        if let Some(telegram) = &self.telegram_service {
            let notification = format!(
                "🔔 NEW TOKEN DETECTED 🔔\n\n\
                🪙 Token: {} ({})\n\
                💲 Initial Price: {} SOL\n\
                💰 Launcher Balance: {} SOL\n\
                ⏱️ Created: {}\n\n\
                🔗 Explorer: https://explorer.solana.com/address/{}",
                token_name, token_symbol, price, launcher_sol, 
                Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
                token_mint
            );
            
            if let Err(e) = telegram.send_notification(&notification).await {
                self.logger.log(format!(
                    "Failed to send notification for new token {}: {}", 
                    token_mint, e
                ).red().to_string());
            }
        }
        
        // Add token to sell manager for monitoring
        let mut sell_manager = self.sell_manager.lock().unwrap();
        sell_manager.add_token_strategy(token_mint.to_string(), price, 0.0, 6);
        
        self.logger.log(format!(
            "Added token {} to sell manager for monitoring", 
            token_mint
        ).green().to_string());
        
        Ok(())
    }

    /// Handle token transaction (buy/sell) events
    async fn handle_token_transaction(
        &self, 
        _tx_update: &SubscribeUpdateTransaction,
        token_mint: &str,
        parsed_tx: &ParsedTransactionInfo
    ) -> Result<()> {
        self.logger.log(format!("Processing token transaction for: {}", token_mint).cyan().to_string());
        
        // Use the parsed transaction data
        let is_buy = matches!(parsed_tx.transaction_type, TransactionType::Buy);
        let amount = parsed_tx.amount;
        let token_amount = parsed_tx.token_amount;
        let price = parsed_tx.price_per_token;
        
        // Check if token is in whitelist
        if !self.token_list_manager.is_whitelisted(token_mint).await {
            self.logger.log(format!("Token {} not in whitelist, skipping transaction", token_mint).yellow().to_string());
            return Ok(());
        }

        // Update token in the tracker
        if self.token_tracker.is_token_monitored(token_mint) {
            // Update token data
            if let Err(e) = self.token_tracker.update_token(token_mint, price, is_buy).await {
                self.logger.log(format!("Failed to update token on {}: {}", 
                    if is_buy { "buy" } else { "sell" }, e).red().to_string());
            }

            // Get updated token info
            let token_info = {
                let extended_tokens_map = self.token_tracker.get_extended_tokens_map_arc();
                let extended_tokens = extended_tokens_map.lock().unwrap();
                extended_tokens.get(token_mint).map(|t| t.clone_without_task())
            };

            if let Some(token_info) = token_info {
                // Check if token meets criteria for buy
                let meets_criteria = {
                    self.token_tracker.is_token_monitored(token_mint)
                };

                if meets_criteria {
                    // Get a fully owned BuyManager
                    let mut buy_manager_owned = {
                        let guard = self.buy_manager.lock().unwrap();
                        guard.clone()
                    };

                    // Execute buy with owned values
                    let buy_result = buy_manager_owned.execute_buy(
                        token_mint,
                        &self.swapx,
                        &self.config.swap_config,
                        &self.existing_liquidity_pools,
                        &self.telegram_service,
                        &self.telegram_chat_id,
                        &token_info,
                        None,
                    ).await;

                    if let Err(e) = buy_result {
                        self.logger.log(format!("Failed to execute buy for token {}: {}", token_mint, e).red().to_string());
                    }
                }

                // For sell transactions, evaluate selling conditions
                if !is_buy {
                    // Get a fully owned SellManager
                    let mut sell_manager_owned = {
                        let guard = self.sell_manager.lock().unwrap();
                        guard.clone()
                    };

                    // Evaluate sell conditions
                    let _ = sell_manager_owned.evaluate_selling_conditions(
                        token_mint,
                        &self.swapx,
                        &self.config.swap_config,
                        &self.existing_liquidity_pools,
                        &self.telegram_service,
                        &self.telegram_chat_id,
                    ).await;
                }

                // Send notification if configured
                if let Some(telegram) = &self.telegram_service {
                    // Get token symbol if available
                    let token_symbol = self.token_tracker.get_token_symbol(token_mint).await;
                    let pnl = if !is_buy { self.token_tracker.get_token_pnl(token_mint, price).await } else { None };
                    
                    // Send transaction notification
                    if let Err(e) = telegram.send_transaction_notification(
                        if is_buy { "buy" } else { "sell" },
                        token_mint,
                        token_symbol.as_deref(),
                        amount,
                        token_amount,
                        price,
                        &parsed_tx.signature,
                        pnl
                    ).await {
                        self.logger.log(format!("Failed to send transaction notification: {}", e).red().to_string());
                    }
                }
            }
        }
        
        Ok(())
    }
}

// Add Default implementation for ParsedTransactionInfo to support fallback case
impl Default for ParsedTransactionInfo {
    fn default() -> Self {
        Self {
            slot: 0,
            recent_blockhash: SolanaHash::default(),
            signature: String::new(),
            transaction_type: TransactionType::Mint,
            timestamp: SystemTime::now(),
            mint: String::new(),
            amount: 0.0,
            token_amount: 0.0,
            price_per_token: 0.0,
            volume_change: 0,
            market_cap: 0.0,
            bonding_curve: String::new(),
            bonding_curve_info: BondingCurveInfo::default(),
            execution_time_ms: 0,
            block_time: None,
            token_name: None,
            token_symbol: None,
            sender: String::new(),
            launcher_sol_balance: 0.0,
        }
    }
}

/// Helper function to get a regular Logger from an Arc<Logger>
fn get_inner_logger(logger_arc: &Arc<Logger>) -> Logger {
    logger_arc.as_ref().clone()
}

/// Helper function that returns a Logger clone when already working with a Logger
fn clone_logger(logger: &Logger) -> Logger {
    logger.clone()
}