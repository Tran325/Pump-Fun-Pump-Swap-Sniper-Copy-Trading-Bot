use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use reqwest::Client;
use crate::common::logger::Logger;
use colored::Colorize;
use anyhow::{Result, anyhow};
use tokio::time::Duration;
use std::time::Instant;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::env;
use std::collections::HashSet;

// Constant for the config file name
const CONFIG_FILE_NAME: &str = "telegram_config.json";

// Telegram filter settings
#[derive(Clone, Serialize, Deserialize)]
pub struct TelegramFilterSettings {
    pub market_cap: FilterRange<f64>,
    pub volume: FilterRange<f64>,
    pub buy_sell_count: FilterRange<i32>,
    pub sol_invested: f64,
    pub launcher_sol_balance: FilterRange<f64>,
    pub dev_buy_bundle: FilterRange<f64>,
    // Track enabled/disabled status for each filter
    pub market_cap_enabled: bool,
    pub volume_enabled: bool,
    pub buy_sell_count_enabled: bool,
    pub sol_invested_enabled: bool,
    pub launcher_sol_balance_enabled: bool,
    pub dev_buy_bundle_enabled: bool,
}

// Range filter for numeric values
#[derive(Clone, Serialize, Deserialize)]
pub struct FilterRange<T> {
    pub min: T,
    pub max: T,
}

// Load settings from environment variables
impl TelegramFilterSettings {
    pub fn from_env() -> Self {
        // Helper function to parse env var as f64 with default
        fn parse_f64_env(key: &str, default: f64) -> f64 {
            match std::env::var(key) {
                Ok(val) => val.parse::<f64>().unwrap_or(default),
                Err(_) => default,
            }
        }
        
        // Helper function to parse env var as i32 with default
        fn parse_i32_env(key: &str, default: i32) -> i32 {
            match std::env::var(key) {
                Ok(val) => val.parse::<i32>().unwrap_or(default),
                Err(_) => default,
            }
        }
        
        // Helper function to parse env var as bool with default
        fn parse_bool_env(key: &str, default: bool) -> bool {
            match std::env::var(key) {
                Ok(val) => val.parse::<bool>().unwrap_or(default),
                Err(_) => default,
            }
        }
        
        // Try to load from the config file first
        if let Ok(settings) = Self::load_from_file() {
            return settings;
        }
        
        // Fall back to environment variables if file doesn't exist
        Self {
            market_cap: FilterRange {
                min: parse_f64_env("MIN_MARKET_CAP", 8.0),
                max: parse_f64_env("MAX_MARKET_CAP", 15.0),
            },
            volume: FilterRange {
                min: parse_f64_env("MIN_VOLUME", 5.0),
                max: parse_f64_env("MAX_VOLUME", 12.0),
            },
            buy_sell_count: FilterRange {
                min: parse_i32_env("MIN_NUMBER_OF_BUY_SELL", 50),
                max: parse_i32_env("MAX_NUMBER_OF_BUY_SELL", 2000),
            },
            sol_invested: parse_f64_env("SOL_INVESTED", 1.0),
            launcher_sol_balance: FilterRange {
                min: parse_f64_env("MIN_LAUNCHER_SOL_BALANCE", 0.0),
                max: parse_f64_env("MAX_LAUNCHER_SOL_BALANCE", 1.0),
            },
            dev_buy_bundle: FilterRange {
                min: parse_f64_env("MIN_DEV_BUY", 5.0),
                max: parse_f64_env("MAX_DEV_BUY", 30.0),
            },
            // Set all filters to true by default
            market_cap_enabled: parse_bool_env("MARKET_CAP_ENABLED", true),
            volume_enabled: parse_bool_env("VOLUME_ENABLED", true), 
            buy_sell_count_enabled: parse_bool_env("BUY_SELL_COUNT_ENABLED", true),
            sol_invested_enabled: parse_bool_env("SOL_INVESTED_ENABLED", true),
            launcher_sol_balance_enabled: parse_bool_env("LAUNCHER_SOL_ENABLED", true),
            dev_buy_bundle_enabled: parse_bool_env("DEV_BUY_ENABLED", true),
        }
    }
    
    // Get the path to the config file
    fn get_config_path() -> PathBuf {
        // Try to use the executable directory first
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let config_path = exe_dir.join(CONFIG_FILE_NAME);
                return config_path;
            }
        }
        
        // Fall back to current directory
        PathBuf::from(CONFIG_FILE_NAME)
    }
    
    // Save settings to a file
    pub fn save_to_file(&self) -> Result<()> {
        let config_path = Self::get_config_path();
        let config_json = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, config_json)?;
        Ok(())
    }
    
    // Load settings from a file
    pub fn load_from_file() -> Result<Self> {
        let config_path = Self::get_config_path();
        if !config_path.exists() {
            return Err(anyhow!("Config file doesn't exist at {:?}", config_path));
        }
        
        let config_json = fs::read_to_string(config_path)?;
        let settings = serde_json::from_str(&config_json)?;
        Ok(settings)
    }
}

// Default implementation for Telegram filter settings
impl Default for TelegramFilterSettings {
    fn default() -> Self {
        Self::from_env()
    }
}

// Token information for filtered notifications
#[derive(Clone)]
pub struct TokenInfo {
    pub address: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub market_cap: Option<f64>,
    pub volume: Option<f64>,
    pub buy_sell_count: Option<i32>,
    pub dev_buy_amount: Option<f64>,
    pub launcher_sol_balance: Option<f64>,
    pub bundle_check: Option<bool>,
    pub ath: Option<f64>,
    // New fields to match screenshot
    pub buy_ratio: Option<i32>,
    pub sell_ratio: Option<i32>,
    pub total_buy_sell: Option<i32>,
    pub volume_buy: Option<f64>,
    pub volume_sell: Option<f64>,
    pub token_price: Option<f64>,
    pub dev_wallet: Option<String>,
    pub sol_balance: Option<f64>,
}

// Message to be sent to Telegram
#[derive(Serialize, Deserialize)]
struct TelegramMessage {
    chat_id: String,
    text: String,
    parse_mode: String,
}

// Create a separate struct for deserialization
#[derive(Deserialize)]
pub struct TelegramMessageReceived {
    pub message_id: i64,
    pub chat: TelegramChat,
    pub text: Option<String>,
}

// Telegram keyboard button
#[derive(Serialize, Deserialize)]
struct InlineKeyboardButton {
    text: String,
    callback_data: String,
}

// Telegram keyboard markup
#[derive(Serialize)]
struct InlineKeyboardMarkup {
    inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

// Message with keyboard to be sent to Telegram
#[derive(Serialize)]
struct TelegramMessageWithKeyboard {
    chat_id: String,
    text: String,
    parse_mode: String,
    reply_markup: InlineKeyboardMarkup,
}

// Callback query format from Telegram
#[derive(Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub callback_query: Option<CallbackQuery>,
    pub message: Option<TelegramMessageReceived>,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub data: String,
    pub message: TelegramMessageReceived,
}

#[derive(Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

#[derive(Deserialize)]
pub struct TelegramMessageObject {
    pub message_id: i64,
    pub chat: TelegramChat,
}

#[derive(Deserialize)]
pub struct TelegramChat {
    pub id: i64,
    pub first_name: Option<String>,
    pub username: Option<String>,
}

// Telegram service for sending notifications and handling UI commands
#[derive(Clone)]
pub struct TelegramService {
    bot_token: String,
    chat_id: String,
    client: Client,
    filter_settings: Arc<Mutex<TelegramFilterSettings>>,
    logger: Logger,
    default_chat_id: String,
    last_notification_time: Instant,
    notification_interval: Duration,
    notified_tokens: Arc<Mutex<HashSet<String>>>, // Track tokens for which we've sent notifications
}

impl TelegramService {
    // Create a new Telegram service
    pub fn new(bot_token: String, chat_id: String, notification_interval_secs: u64) -> Self {
        let logger = Logger::new("[TELEGRAM] => ".blue().bold().to_string());
        let client = Client::new();
        
        // Create filter settings from environment variables
        let filter_settings = Arc::new(Mutex::new(TelegramFilterSettings::from_env()));
        
        // Load or create notification configuration
        let config_path = TelegramFilterSettings::get_config_path();
        if !config_path.exists() {
            if let Err(e) = filter_settings.lock().unwrap().save_to_file() {
                logger.log(format!("Error saving initial filter settings to file: {}", e));
            }
        }
        
        logger.log(format!("Initialized Telegram service with chat ID: {}", chat_id));
        
        Self {
            bot_token,
            chat_id: chat_id.clone(),
            client,
            filter_settings,
            logger,
            default_chat_id: chat_id,
            last_notification_time: Instant::now(),
            notification_interval: Duration::from_secs(notification_interval_secs),
            notified_tokens: Arc::new(Mutex::new(HashSet::new())), // Initialize empty set of notified tokens
        }
    }

    // Public method to get a clone of the current filter settings
    pub fn get_filter_settings(&self) -> TelegramFilterSettings {
        self.filter_settings.lock().unwrap().clone()
    }

    // Send a notification about a filtered token
    pub async fn send_token_notification(&self, token: &TokenInfo) -> Result<()> {
        // First, check if the token passes filters and should be notified
        if !self.token_passes_filters(token) {
            return Ok(());
        }
        
        // Mark this token as notified to avoid duplicate notifications
        if let Ok(mut notified_tokens) = self.notified_tokens.lock() {
            notified_tokens.insert(token.address.clone());
        }
        
        // Prepare token name display
        let token_name_display = match (&token.name, &token.symbol) {
            (Some(name), Some(symbol)) => format!("{} ({})", name, symbol),
            (Some(name), None) => name.clone(),
            (None, Some(symbol)) => symbol.clone(),
            (None, None) => "Unknown Token".to_string(),
        };
        
        // Format buy/sell metrics
        let buy_sell_metrics = if let (Some(buy), Some(sell), Some(total)) = (token.buy_ratio, token.sell_ratio, token.total_buy_sell) {
            format!("🔄 <b>Buy/Sell Activity:</b>\n├ Buy: {}% ({} txs)\n├ Sell: {}% ({} txs)\n└ Total: {} transactions", 
                buy, 
                (total as f64 * buy as f64 / 100.0).round() as i32,
                sell,
                (total as f64 * sell as f64 / 100.0).round() as i32,
                total
            )
        } else {
            format!("🔄 <b>Buy/Sell Activity:</b> Data not available")
        };
        
        // Format volume metrics
        let volume_metrics = if let (Some(vol), Some(vol_buy), Some(vol_sell)) = (token.volume, token.volume_buy, token.volume_sell) {
            format!("💰 <b>Volume Data:</b>\n├ Total: {:.2} SOL\n├ Buy: {:.2} SOL\n└ Sell: {:.2} SOL", 
                vol, 
                vol_buy,
                vol_sell
            )
        } else if let Some(vol) = token.volume {
            format!("💰 <b>Volume:</b> {:.2} SOL", vol)
        } else {
            format!("💰 <b>Volume:</b> Data not available")
        };
        
        // Dev wallet information
        let dev_wallet_info = if let Some(dev_wallet) = &token.dev_wallet {
            format!("👨‍💻 <b>Dev Wallet:</b> <code>{}</code>", dev_wallet)
        } else {
            "".to_string()
        };
        
        // Dev buy information
        let dev_buy_info = if let Some(dev_buy) = token.dev_buy_amount {
            format!("💸 <b>Dev Buy:</b> {:.2} SOL", dev_buy)
        } else {
            "".to_string()
        };
        
        // Launcher SOL balance
        let launcher_sol_info = if let Some(launcher_sol) = token.launcher_sol_balance {
            format!("💼 <b>Launcher SOL Balance:</b> {:.2} SOL", launcher_sol)
        } else {
            "".to_string()
        };
        
        // Market cap information
        let market_cap_info = if let Some(market_cap) = token.market_cap {
            format!("📊 <b>Market Cap:</b> {:.2}K", market_cap)
        } else {
            "".to_string()
        };
        
        // Generate the message
        let message = format!(
            "🔔 <b>TOKEN ACTIVITY ALERT</b> 🔔\n\n\
            <b>🪙 Token:</b> {} \n\
            <b>📝 Address:</b> <code>{}</code>\n\n\
            {}\n\n\
            {}\n\n\
            {}\n\
            {}\n\
            {}\n\
            {}\n\n\
            ⚠️ <b>This token passes all filter criteria:</b>\n\
            ├ Market Cap: {}\n\
            ├ Volume: {}\n\
            ├ Buy/Sell Count: {}\n\
            ├ Token Lifetime > {} ms\n\
            └ Amount in time window > {} SOL\n\n\
            <i>This token meets all your criteria for potential buying.</i>",
            token_name_display,
            token.address,
            buy_sell_metrics,
            volume_metrics,
            market_cap_info,
            dev_buy_info,
            launcher_sol_info,
            dev_wallet_info,
            self.filter_settings.lock().unwrap().market_cap_enabled,
            self.filter_settings.lock().unwrap().volume_enabled,
            self.filter_settings.lock().unwrap().buy_sell_count_enabled,
            std::env::var("MIN_LAST_TIME").unwrap_or_else(|_| "200000".to_string()),
            std::env::var("LIMIT_BUY_AMOUNT_IN_LIMIT_WAIT_TIME").unwrap_or_else(|_| "15".to_string())
        );
        
        // Create buttons for trading actions
        let mut keyboard = Vec::new();
        
        // Add Buy button
        keyboard.push(vec![InlineKeyboardButton {
            text: "🟢 Buy Token".to_string(),
            callback_data: format!("buy:{}", token.address),
        }]);
        
        // Add Set Take Profit and Stop Loss buttons
        keyboard.push(vec![
            InlineKeyboardButton {
                text: "📈 Set Take Profit".to_string(),
                callback_data: format!("settp:{}", token.address),
            },
            InlineKeyboardButton {
                text: "📉 Set Stop Loss".to_string(),
                callback_data: format!("setsl:{}", token.address),
            },
        ]);
        
        // Add Sell button
        keyboard.push(vec![
            InlineKeyboardButton {
                text: "🔴 Sell All".to_string(),
                callback_data: format!("sell:{}", token.address),
            },
        ]);
        
        // Create message with keyboard
        let message_with_keyboard = TelegramMessageWithKeyboard {
            chat_id: self.chat_id.clone(),
            text: message,
            parse_mode: "HTML".to_string(),
            reply_markup: InlineKeyboardMarkup {
                inline_keyboard: keyboard,
            },
        };
        
        // Send the message
        match self.client.post(format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token))
            .json(&message_with_keyboard)
            .send()
            .await {
                Ok(response) => {
                    if response.status().is_success() {
                        self.logger.log(format!("Sent notification about token {}", token.address).green().to_string());
                        Ok(())
                    } else {
                        let error = format!("Failed to send notification: HTTP {}", response.status());
                        self.logger.log(error.clone().red().to_string());
                        Err(anyhow!(error))
                    }
                },
                Err(e) => {
                    let error = format!("Error sending notification: {}", e);
                    self.logger.log(error.clone().red().to_string());
                    Err(anyhow!(error))
                }
            }
    }

    // Send the filter settings UI to Telegram
    pub async fn send_filter_settings_ui(&self) -> Result<()> {
        // Create the message text and keyboard data first, before any await points
        let (message, keyboard) = {
            let settings = self.filter_settings.lock().unwrap();
            
            let message_text = format!(
                "⚙️ <b>Token Filter Settings</b>\n\n\
                ❄️ <b>Monitoring Market Cap:</b> {}\n\
                📈 Min MC: {}K  📉 Max MC: {}K\n\n\
                🔥 <b>Monitoring Volumes:</b> {}\n\
                📈 Min: {}K  📉 Max: {}K\n\n\
                ☀️ <b>Monitoring Number of Buy/Sell:</b> {}\n\
                📈 Min: {}  📉 Max: {}\n\n\
                💸 <b>Monitoring Sol Invested:</b> {}\n\
                ⚡ Invest: {} Sol\n\n\
                🔴 <b>Monitoring Launcher Sol Balance:</b> {}\n\
                📈 Min: {} Sol  📉 Max: {} Sol\n\n\
                ❄️ <b>Dev Buy & Bundle Check:</b> {}\n\
                📈 Min: {} Sol  📉 Max: {} Sol\n\
                🔄 <b>Bundle Check Required:</b> {}",
                if settings.market_cap_enabled { "✅" } else { "❌" },
                settings.market_cap.min, settings.market_cap.max,
                if settings.volume_enabled { "✅" } else { "❌" },
                settings.volume.min, settings.volume.max,
                if settings.buy_sell_count_enabled { "✅" } else { "❌" },
                settings.buy_sell_count.min, settings.buy_sell_count.max,
                if settings.sol_invested_enabled { "✅" } else { "❌" },
                settings.sol_invested,
                if settings.launcher_sol_balance_enabled { "✅" } else { "❌" },
                settings.launcher_sol_balance.min, settings.launcher_sol_balance.max,
                if settings.dev_buy_bundle_enabled { "✅" } else { "❌" },
                settings.dev_buy_bundle.min, settings.dev_buy_bundle.max,
                if let Ok(bundle_check) = std::env::var("BUNDLE_CHECK") {
                    if bundle_check.to_lowercase() == "true" { "✅" } else { "❌" }
                } else {
                    "❌"
                }
            );
            
            let keyboard_data = vec![
                // Market Cap row
                vec![
                    InlineKeyboardButton {
                        text: format!("Market Cap: {}", if settings.market_cap_enabled { "✅" } else { "❌" }),
                        callback_data: "toggle_market_cap".to_string(),
                    },
                ],
                // Volume row
                vec![
                    InlineKeyboardButton {
                        text: format!("Volumes: {}", if settings.volume_enabled { "✅" } else { "❌" }),
                        callback_data: "toggle_volume".to_string(),
                    },
                ],
                // Buy/Sell Count row
                vec![
                    InlineKeyboardButton {
                        text: format!("Buy/Sell Count: {}", if settings.buy_sell_count_enabled { "✅" } else { "❌" }),
                        callback_data: "toggle_buy_sell".to_string(),
                    },
                ],
                // Sol Invested row
                vec![
                    InlineKeyboardButton {
                        text: format!("Sol Invested: {}", if settings.sol_invested_enabled { "✅" } else { "❌" }),
                        callback_data: "toggle_sol_invested".to_string(),
                    },
                ],
                // Launcher Sol Balance row
                vec![
                    InlineKeyboardButton {
                        text: format!("Launcher Sol Balance: {}", if settings.launcher_sol_balance_enabled { "✅" } else { "❌" }),
                        callback_data: "toggle_launcher_sol".to_string(),
                    },
                ],
                // Dev Buy Check row
                vec![
                    InlineKeyboardButton {
                        text: format!("Dev Buy & Bundle: {}", if settings.dev_buy_bundle_enabled { "✅" } else { "❌" }),
                        callback_data: "toggle_dev_buy".to_string(),
                    },
                ],
            ];
            
            (message_text, keyboard_data)
        }; // MutexGuard is dropped here

        // Now we can proceed with async operations
        let msg = TelegramMessageWithKeyboard {
            chat_id: self.chat_id.clone(),
            text: message,
            parse_mode: "HTML".to_string(),
            reply_markup: InlineKeyboardMarkup {
                inline_keyboard: keyboard,
            },
        };

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let res = self.client.post(&url).json(&msg).send().await?;
        
        if !res.status().is_success() {
            return Err(anyhow!("Failed to send message to Telegram: {}", res.status()));
        }
        
        self.logger.log("Sent filter settings UI to Telegram".to_string());
        Ok(())
    }

    // Check if a token passes all enabled filters
    pub fn token_passes_filters(&self, token: &TokenInfo) -> bool {
        // Get our filter settings
        let filter_settings = self.filter_settings.lock().unwrap();
        
        // First, check if the token has already been notified to avoid duplicate notifications
        if let Ok(notified_tokens) = self.notified_tokens.lock() {
            if notified_tokens.contains(&token.address) {
                // Skip this token as we've already notified about it
                return false;
            }
        }
        
        // IMPORTANT: First check if it has enough buy and sell activity
        // Since this is a mandatory check per user requirements
        if let Some(buy_sell_count) = token.buy_sell_count {
            if filter_settings.buy_sell_count_enabled {
                if buy_sell_count < filter_settings.buy_sell_count.min || 
                   buy_sell_count > filter_settings.buy_sell_count.max {
                    self.logger.log(format!(
                        "Token {} failed buy/sell count filter: {} not in range {}-{}", 
                        token.address,
                        buy_sell_count,
                        filter_settings.buy_sell_count.min,
                        filter_settings.buy_sell_count.max
                    ).yellow().to_string());
                    return false;
                }
            }
        } else {
            // No buy/sell count data available and this filter is enabled
            if filter_settings.buy_sell_count_enabled {
                self.logger.log(format!(
                    "Token {} failed buy/sell count filter: no data available", 
                    token.address
                ).yellow().to_string());
                return false;
            }
        }
        
        // Check token lifetime if MIN_LAST_TIME is set
        let _min_last_time = std::env::var("MIN_LAST_TIME")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(200000); // Default to 200 seconds (200,000 ms)
        
        // Token age is stored in token_tracker, so we can't access it directly here
        // We'll rely on token_tracker ensuring this check before sending for notification
        
        // Apply market cap filter
        if filter_settings.market_cap_enabled {
            if let Some(market_cap) = token.market_cap {
                if market_cap < filter_settings.market_cap.min || market_cap > filter_settings.market_cap.max {
                    self.logger.log(format!(
                        "Token {} failed market cap filter: {:.2} not in range {}-{}", 
                        token.address,
                        market_cap,
                        filter_settings.market_cap.min,
                        filter_settings.market_cap.max
                    ).yellow().to_string());
                    return false;
                }
            } else {
                // No market cap data available and this filter is enabled
                self.logger.log(format!(
                    "Token {} failed market cap filter: no data available", 
                    token.address
                ).yellow().to_string());
                return false;
            }
        }
        
        // Apply volume filter
        if filter_settings.volume_enabled {
            if let Some(volume) = token.volume {
                if volume < filter_settings.volume.min || volume > filter_settings.volume.max {
                    self.logger.log(format!(
                        "Token {} failed volume filter: {:.2} not in range {}-{}", 
                        token.address,
                        volume,
                        filter_settings.volume.min,
                        filter_settings.volume.max
                    ).yellow().to_string());
                    return false;
                }
            } else {
                // No volume data available and this filter is enabled
                self.logger.log(format!(
                    "Token {} failed volume filter: no data available", 
                    token.address
                ).yellow().to_string());
                return false;
            }
        }
        
        // Apply launcher sol balance filter
        if filter_settings.launcher_sol_balance_enabled {
            if let Some(launcher_sol) = token.launcher_sol_balance {
                if launcher_sol < filter_settings.launcher_sol_balance.min || 
                   launcher_sol > filter_settings.launcher_sol_balance.max {
                    self.logger.log(format!(
                        "Token {} failed launcher SOL filter: {:.2} not in range {}-{}", 
                        token.address,
                        launcher_sol,
                        filter_settings.launcher_sol_balance.min,
                        filter_settings.launcher_sol_balance.max
                    ).yellow().to_string());
                    return false;
                }
            } else {
                // No launcher SOL data available and this filter is enabled
                self.logger.log(format!(
                    "Token {} failed launcher SOL filter: no data available", 
                    token.address
                ).yellow().to_string());
                return false;
            }
        }
        
        // Apply dev buy amount filter
        if filter_settings.dev_buy_bundle_enabled {
            if let Some(dev_buy) = token.dev_buy_amount {
                if dev_buy < filter_settings.dev_buy_bundle.min || 
                   dev_buy > filter_settings.dev_buy_bundle.max {
                    self.logger.log(format!(
                        "Token {} failed dev buy filter: {:.2} SOL not in range {}-{}", 
                        token.address,
                        dev_buy,
                        filter_settings.dev_buy_bundle.min,
                        filter_settings.dev_buy_bundle.max
                    ).yellow().to_string());
                    return false;
                }
            } else {
                // No dev buy data available and this filter is enabled
                self.logger.log(format!(
                    "Token {} failed dev buy filter: no data available", 
                    token.address
                ).yellow().to_string());
                return false;
            }
        }
        
        // Apply SOL invested filter
        if filter_settings.sol_invested_enabled {
            if let Some(volume) = token.volume {
                if volume < filter_settings.sol_invested {
                    self.logger.log(format!(
                        "Token {} failed SOL invested filter: {:.2} SOL < required {}", 
                        token.address,
                        volume,
                        filter_settings.sol_invested
                    ).yellow().to_string());
                    return false;
                }
            } else {
                // No volume data available and this filter is enabled
                self.logger.log(format!(
                    "Token {} failed SOL invested filter: no data available", 
                    token.address
                ).yellow().to_string());
                return false;
            }
        }
        
        // Check both the buy/sell ratio and limit wait time volume
        let _limit_wait_time = std::env::var("LIMIT_WAIT_TIME")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120000); // Default to 120 seconds (120,000 ms)
        
        let limit_buy_amount = std::env::var("LIMIT_BUY_AMOUNT_IN_LIMIT_WAIT_TIME")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(15.0); // Default to 15 SOL
        
        // Check if volume meets the minimum required during the wait time
        if let Some(volume) = token.volume {
            if volume < limit_buy_amount {
                self.logger.log(format!(
                    "Token {} failed limit volume filter: {:.2} SOL < required {}", 
                    token.address,
                    volume,
                    limit_buy_amount
                ).yellow().to_string());
                return false;
            }
        }
        
        // If we got here, token passes all enabled filters
        self.logger.log(format!(
            "Token {} passes all filters", 
            token.address
        ).green().to_string());
        
        true
    }

    // Toggle a specific filter on/off and save to environment
    pub fn toggle_filter(&self, filter_name: &str) -> Result<bool> {
        let mut settings = self.filter_settings.lock().unwrap();
        
        let (enabled, env_var) = match filter_name {
            "market_cap" => {
                settings.market_cap_enabled = !settings.market_cap_enabled;
                (settings.market_cap_enabled, "MARKET_CAP_ENABLED")
            },
            "volume" => {
                settings.volume_enabled = !settings.volume_enabled;
                (settings.volume_enabled, "VOLUME_ENABLED")
            },
            "buy_sell" => {
                settings.buy_sell_count_enabled = !settings.buy_sell_count_enabled;
                (settings.buy_sell_count_enabled, "BUY_SELL_COUNT_ENABLED")
            },
            "sol_invested" => {
                settings.sol_invested_enabled = !settings.sol_invested_enabled;
                (settings.sol_invested_enabled, "SOL_INVESTED_ENABLED")
            },
            "launcher_sol" => {
                settings.launcher_sol_balance_enabled = !settings.launcher_sol_balance_enabled;
                (settings.launcher_sol_balance_enabled, "LAUNCHER_SOL_ENABLED")
            },
            "dev_buy" => {
                settings.dev_buy_bundle_enabled = !settings.dev_buy_bundle_enabled;
                (settings.dev_buy_bundle_enabled, "DEV_BUY_ENABLED")
            },
            _ => return Err(anyhow!("Unknown filter: {}", filter_name)),
        };
        
        // Save the settings to a file so they persist
        if let Err(e) = settings.save_to_file() {
            self.logger.log(format!("Failed to save filter settings: {}", e));
        } else {
            self.logger.log(format!("Saved filter settings to file: {} = {}", env_var, enabled));
        }
        
        Ok(enabled)
    }

    // Process callback queries from the Telegram UI
    pub async fn process_callback(&self, callback_data: &str, callback_id: &str) -> Result<()> {
        // First acknowledge the callback to stop the loading indicator
        self.answer_callback_query(callback_id).await?;
        
        // Process the callback and store the result before calling any await points
        let _action = match callback_data {
            "toggle_market_cap" => {
                let enabled = self.toggle_filter("market_cap")?;
                format!("Market Cap filter {}", if enabled { "enabled" } else { "disabled" })
            },
            "toggle_volume" => {
                let enabled = self.toggle_filter("volume")?;
                format!("Volume filter {}", if enabled { "enabled" } else { "disabled" })
            },
            "toggle_buy_sell" => {
                let enabled = self.toggle_filter("buy_sell")?;
                format!("Buy/Sell Count filter {}", if enabled { "enabled" } else { "disabled" })
            },
            "toggle_sol_invested" => {
                let enabled = self.toggle_filter("sol_invested")?;
                format!("SOL Invested filter {}", if enabled { "enabled" } else { "disabled" })
            },
            "toggle_launcher_sol" => {
                let enabled = self.toggle_filter("launcher_sol")?;
                format!("Launcher SOL Balance filter {}", if enabled { "enabled" } else { "disabled" })
            },
            "toggle_dev_buy" => {
                let enabled = self.toggle_filter("dev_buy")?;
                format!("Dev Buy & Bundle Check filter {}", if enabled { "enabled" } else { "disabled" })
            },
            "stop_all" => {
                // Use a block to ensure the mutex guard is dropped before the await point
                {
                    let mut settings = self.filter_settings.lock().unwrap();
                    settings.market_cap_enabled = false;
                    settings.volume_enabled = false;
                    settings.buy_sell_count_enabled = false;
                    settings.sol_invested_enabled = false;
                    settings.launcher_sol_balance_enabled = false;
                    settings.dev_buy_bundle_enabled = false;
                    
                    // Save the settings to file for persistence
                    if let Err(e) = settings.save_to_file() {
                        self.logger.log(format!("Failed to save filter settings: {}", e));
                    } else {
                        self.logger.log("Saved all disabled filters to file".to_string());
                    }
                } // MutexGuard is dropped here
                
                "All filters disabled".to_string()
            },
            _ => format!("Unknown callback: {}", callback_data),
        };
        
        // Update the UI to reflect the changes
        self.send_filter_settings_ui().await?;
        
        Ok(())
    }
    
    // Answer a callback query to stop the loading indicator
    async fn answer_callback_query(&self, callback_query_id: &str) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/answerCallbackQuery", self.bot_token);
        let params = serde_json::json!({
            "callback_query_id": callback_query_id
        });
        
        let res = self.client.post(&url).json(&params).send().await?;
        
        if !res.status().is_success() {
            return Err(anyhow!("Failed to answer callback query: {}", res.status()));
        }
        
        Ok(())
    }
    
    // Poll for updates to process callbacks
    pub async fn start_polling(&self) {
        let logger = self.logger.clone();
        logger.log("Starting Telegram update polling...".to_string());
        
        let client = self.client.clone();
        let bot_token = self.bot_token.clone();
        let chat_id = self.chat_id.clone();
        let service = self.clone();
        
        tokio::spawn(async move {
            let mut last_update_id = 0;
            let mut last_config_check = Instant::now();
            let config_check_interval = Duration::from_secs(30); // Check config every 30 seconds
            
            loop {
                // Check if config file has changed and reload if needed
                if last_config_check.elapsed() >= config_check_interval {
                    if let Err(e) = service.reload_config_if_changed().await {
                        eprintln!("Error checking config file: {}", e);
                    }
                    last_config_check = Instant::now();
                }
                
                let url = format!(
                    "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30",
                    bot_token, last_update_id + 1
                );
                
                match client.get(&url).send().await {
                    Ok(response) => {
                        if let Ok(updates) = response.json::<serde_json::Value>().await {
                            if let Some(results) = updates["result"].as_array() {
                                for update in results {
                                    if let Ok(update) = serde_json::from_value::<TelegramUpdate>(update.clone()) {
                                        last_update_id = update.update_id;
                                        
                                        if let Some(callback) = update.callback_query {
                                            // Check if this callback is for our chat
                                            if callback.message.chat.id.to_string() == chat_id {
                                                if let Err(e) = service.process_callback(&callback.data, &callback.id).await {
                                                    eprintln!("Error processing callback: {}", e);
                                                }
                                            }
                                        } else if let Some(message) = update.message {
                                            // Handle regular messages if needed
                                            if message.chat.id.to_string() == chat_id {
                                                if let Some(text) = message.text {
                                                    match text.as_str() {
                                                        "/start" | "/filters" => {
                                                            if let Err(e) = service.send_filter_settings_ui().await {
                                                                eprintln!("Error sending filter UI: {}", e);
                                                            }
                                                        },
                                                        "/config" => {
                                                            // Send config file path
                                                            let config_path = TelegramFilterSettings::get_config_path();
                                                            let msg = format!(
                                                                "<b>📁 Configuration File Location</b>\n\n\
                                                                Path: <code>{}</code>\n\n\
                                                                <i>This file is automatically updated when you toggle settings in the bot.</i>",
                                                                config_path.display()
                                                            );
                                                            if let Err(e) = service.send_message(&chat_id, &msg, "HTML").await {
                                                                eprintln!("Error sending config path: {}", e);
                                                            }
                                                        },
                                                        _ => {}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("Error polling Telegram updates: {}", e);
                    }
                }
                
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }

    // Add a method to reload settings from file if changed
    async fn reload_config_if_changed(&self) -> Result<()> {
        // Only proceed if the config file exists
        let config_path = TelegramFilterSettings::get_config_path();
        if !config_path.exists() {
            return Ok(());
        }
        
        // Try to read the file to check if it's changed
        match TelegramFilterSettings::load_from_file() {
            Ok(new_settings) => {
                // Compare and update in a separate block to drop the mutex guard
                let should_update = {
                    let current_settings = self.filter_settings.lock().unwrap();
                    
                    // Compare the serialized versions to see if they're different
                    let current_json = serde_json::to_string(&*current_settings)?;
                    let new_json = serde_json::to_string(&new_settings)?;
                    
                    current_json != new_json
                }; // MutexGuard is dropped here
                
                if should_update {
                    self.logger.log("Config file changed, reloading settings".green().to_string());
                    // Use a new block to limit the scope of the mutex guard
                    {
                        let mut current_settings = self.filter_settings.lock().unwrap();
                        *current_settings = new_settings;
                    } // MutexGuard is dropped here
                }
            },
            Err(e) => {
                self.logger.log(format!("Error loading config file: {}", e).red().to_string());
            }
        }
        
        Ok(())
    }

    pub async fn send_notification(&self, message: &str) -> Result<()> {
        // Check if we should throttle notifications
        if self.last_notification_time.elapsed() < self.notification_interval {
            return Ok(());
        }
        
        self.send_message(&self.default_chat_id, message, "HTML").await
    }
    
    pub async fn send_message(&self, chat_id: &str, message: &str, parse_mode: &str) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        
        let response = self.client
            .post(&url)
            .json(&json!({
                "chat_id": chat_id,
                "text": message,
                "parse_mode": parse_mode
            }))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send telegram message: {}", e))?;
            
        if !response.status().is_success() {
            return Err(anyhow!("Telegram API returned error: {}", response.status()));
        }
        
        Ok(())
    }

    // Reset notification status for a token (could be used if needed)
    pub fn reset_token_notification_status(&self, token_address: &str) -> Result<()> {
        let mut notified_tokens = self.notified_tokens.lock().unwrap();
        if notified_tokens.remove(token_address) {
            self.logger.log(format!("Reset notification status for token: {}", token_address));
        }
        Ok(())
    }
    
    // Get list of tokens that have been notified
    pub fn get_notified_tokens(&self) -> Vec<String> {
        let notified_tokens = self.notified_tokens.lock().unwrap();
        notified_tokens.iter().cloned().collect()
    }

    /// Send a detailed transaction notification with buy/sell details
    pub async fn send_transaction_notification(
        &self,
        transaction_type: &str,
        token_mint: &str,
        token_symbol: Option<&str>,
        amount_sol: f64,
        token_amount: f64,
        price_per_token: f64,
        transaction_hash: &str,
        pnl: Option<f64>,
    ) -> Result<()> {
        // Create a more detailed message with transaction information
        let token_display = if let Some(symbol) = token_symbol {
            format!("`{}` ({})", token_mint, symbol)
        } else {
            format!("`{}`", token_mint)
        };

        let emoji = match transaction_type.to_lowercase().as_str() {
            "buy" => "✅",
            "sell" => "🟥",
            _ => "🔄",
        };

        // Build the message with the available information
        let mut message = format!(
            "{} <b>{} Transaction Details</b>:\n\n\
            🪙 Token: {}\n\
            💰 Amount: {} SOL\n\
            🔢 Token Amount: {} tokens\n\
            💲 Price: {} SOL per token\n",
            emoji, transaction_type.to_uppercase(), token_display, amount_sol, token_amount, price_per_token
        );

        // Add PNL information if available (for sell transactions)
        if let Some(pnl_value) = pnl {
            let pnl_emoji = if pnl_value >= 0.0 { "🟢" } else { "🔴" };
            message.push_str(&format!("📊 PNL: {} {}%\n", pnl_emoji, pnl_value));
        }

        // Add transaction link
        message.push_str(&format!("🔗 <a href=\"https://solscan.io/tx/{}\">View Transaction</a>", transaction_hash));

        // Send the notification
        self.send_message(&self.chat_id, &message, "HTML").await
    }
} 