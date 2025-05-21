// use std::hash::Hash;
use anchor_lang::prelude::Pubkey;
use anchor_client::solana_sdk::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use crate::common::{
    blacklist::Blacklist,
    config::{AppState, SwapConfig},
};
use spl_associated_token_account::get_associated_token_address;
use solana_program_pack::Pack;
use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;
use anyhow::Result;
use std::str::FromStr;
use base64;
use anchor_client::solana_client::rpc_client::RpcClient;
use spl_token::state::Mint;
use crate::dex::pump_fun::{get_pda, PUMP_PROGRAM};
// PumpFun constants
pub const PUMPFUN_CREATE_DATA_PREFIX: &str = "Program data: G3KpTd7rY3Y";
pub const PUMP_FUN_BUY_OR_SELL_PROGRAM_DATA_PREFIX: &str = "Program data: vdt/007mYe";
pub const INITIAL_VIRTUAL_SOL_RESERVES: u64 = 1_000_000_000; // 1 SOL in lamports
pub const INITIAL_VIRTUAL_TOKEN_RESERVES: u64 = 1_000_000_000_000; // 1 trillion tokens

// Type definition for RequestItem
pub type RequestItem = String;

#[derive(Clone, Debug)]
pub struct BondingCurveInfo {
    pub bonding_curve: Pubkey,
    pub new_virtual_sol_reserve: u64,
    pub new_virtual_token_reserve: u64,
}

#[derive(Clone, Debug)]
pub enum TransactionType {
    Mint,
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct ParsedTransactionInfo {
    // Basic Transaction Info
    pub slot: u64,
    pub recent_blockhash: Hash,
    pub signature: String,
    pub transaction_type: TransactionType,
    pub timestamp: SystemTime,
    pub mint: String,

    pub amount: f64,                 // Transaction amount in SOL
    pub token_amount: f64,           // Amount of tokens involved
    pub price_per_token: f64,        // Price per token in SOL
    
    pub volume_change: i64,          // Volume change from this transaction
    pub market_cap: f64,             // Market cap at time of transaction
    // Pool Info
    pub bonding_curve: String,
    pub bonding_curve_info: BondingCurveInfo,
    // Performance Metrics
    pub execution_time_ms: u64,      // Transaction execution time
    pub block_time: Option<i64>,     // Block timestamp
    
    // Added missing fields needed by enhanced_monitor.rs
    pub token_name: Option<String>,
    pub token_symbol: Option<String>,
    pub sender: String,
    pub launcher_sol_balance: f64,
}

#[derive(Debug, Clone)]
pub struct TokenBalance {
    pub account_index: usize,
    pub mint: String,
    pub amount: f64,
    pub decimals: u8,
    pub owner: String,
    pub program_id: String,
}

pub struct FilterConfig {
    pub program_ids: Vec<String>,
    pub _instruction_discriminators: Vec<&'static [u8]>,
}

lazy_static::lazy_static! {
    static ref COUNTER: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));
    static ref SOLD: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));
}

impl ParsedTransactionInfo {
    pub fn from_json(tx_update: &SubscribeUpdateTransaction) -> Result<Self, String> {
        let tx_info = tx_update.transaction.as_ref()
            .ok_or_else(|| "No transaction info".to_string())?;
        
        let meta = tx_info.meta.as_ref()
            .ok_or_else(|| "No transaction metadata".to_string())?;
            
        let message = tx_info.transaction.as_ref()
            .and_then(|tx| tx.message.as_ref())
            .ok_or_else(|| "No transaction message".to_string())?;

            let recent_blockhash_slice = &tx_info
            .transaction
            .as_ref()
            .ok_or_else(|| "Failed to get transaction".to_string())?
            .message
            .as_ref()
            .ok_or_else(|| "Failed to get message".to_string())?
            .recent_blockhash;
        let recent_blockhash = Hash::new(recent_blockhash_slice);

        // Extract basic transaction info
        let signature = bs58::encode(&tx_info.signature).into_string();
        let slot = tx_update.slot;
        let timestamp = SystemTime::now(); // Current time as transaction time
        
        // Parse log messages to determine transaction type and other info
        let log_messages = meta.log_messages.clone();
        
        // Get account keys from the transaction message
        let account_keys = &message.account_keys;

        // Initialize variables for transaction parsing
        let mut token_mint = String::new();
        let mut bonding_curve = String::new();
        let mut _target = String::new();
        let mut token_name = None;
        let mut token_symbol = None;
        let mut token_uri = None;
        let mut _amount = 0u64;
        let mut _max_sol_cost = 0u64;
        let mut _min_sol_cost = 0u64;
        let mut _global = String::new();
        let mut _fee_recipient = String::new();
        let mut _trader = String::new();
        let mut transaction_type = TransactionType::Mint; // Default
        let mut _volume_change = 0_i64;
        let mut token_amount = 0.0;
        let mut sol_amount = 0.0;
        let mut sender = String::new();
        let mut offset = 8; // Discriminator
        let mut _bonding_curve_info = BondingCurveInfo::default();
        let mut price_per_token = 0.0;
        let mut token_supply = 0u64;
        
        // Extract transaction type from logs
        if log_messages.iter().any(|msg| msg.contains("Instruction: Buy")) {
            transaction_type = TransactionType::Buy;
        } else if log_messages.iter().any(|msg| msg.contains("Instruction: Sell")) {
            transaction_type = TransactionType::Sell;
        } else if log_messages.iter().any(|msg| msg.contains("Instruction: Create")) {
            transaction_type = TransactionType::Mint;
        }

        match transaction_type {
            TransactionType::Mint => {
                let encoded_data = log_messages.iter()
                    .find(|msg| msg.starts_with(PUMPFUN_CREATE_DATA_PREFIX))
                    .map_or("", |v| v)
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or("")
                    .to_string();
                let decoded_data = base64::decode(&encoded_data).unwrap();
                let mut token_mint_option: Option<String> = None;
                let mut bonding_curve_option: Option<String> = None;
                let mut target_option: Option<String> = None;
                
                let fields = vec![
                    (&mut token_name, "string"),
                    (&mut token_symbol, "string"),
                    (&mut token_uri, "string"),
                    (&mut token_mint_option, "publicKey"),
                    (&mut bonding_curve_option, "publicKey"),
                    (&mut target_option, "publicKey"),
                ];
                
                for (value, field_type) in fields {
                    if field_type == "string" {
                        if decoded_data.len() < offset + 4 {
                            continue;
                        }
                        
                        let length: u32 = u32::from_le_bytes(decoded_data[offset..offset + 4].try_into().unwrap());
                        offset += 4;
                        
                        if decoded_data.len() < offset + length as usize {
                            continue;
                        }
                        
                        *value = Some(String::from_utf8(decoded_data[offset..offset + length as usize].to_vec()).ok().unwrap());
                        offset += length as usize;
                    } else if field_type == "publicKey" {
                        let key_bytes: [u8; 32] = decoded_data[offset..offset + 32].try_into().unwrap();
                        *value = Some(Pubkey::new_from_array(key_bytes).to_string());
                        offset += 32;
                    }
                }

                if let Some(mint) = token_mint_option {
                    token_mint = mint;
                }
                if let Some(curve) = bonding_curve_option {
                    bonding_curve = curve;
                }
                if let Some(tgt) = target_option {
                    _target = tgt;
                }
                println!("token_mint: {}", token_mint);
                println!("bonding_curve: {}", bonding_curve);
                println!("target: {}", _target);
                // Get initial token supply from mint account
                let client = RpcClient::new(std::env::var("RPC_HTTP").unwrap_or_else(|_| "https://mainnet-fra.fountainhead.land/".to_string()));
                if let Ok(mint_pubkey) = Pubkey::from_str(&token_mint) {
                    if let Ok(mint_account) = client.get_account(&mint_pubkey) {
                        if let Ok(mint) = Mint::unpack_from_slice(&mint_account.data) {
                            token_supply = mint.supply;
                        }
                    }
                }
            },
            TransactionType::Buy | TransactionType::Sell => {
                let encoded_data = log_messages.iter()
                    .find(|msg| msg.starts_with(PUMP_FUN_BUY_OR_SELL_PROGRAM_DATA_PREFIX))
                    .map_or("", |v| v)
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or("")
                    .to_string();
                let decoded_data = base64::decode(&encoded_data).unwrap();

                println!("encoded_data: {}", encoded_data);
               
                if decoded_data.len() >= 40 {  // 8 (discriminator) + 32 (mint)
                    let mint_bytes = &decoded_data[8..40];
                    token_mint = bs58::encode(mint_bytes).into_string();
                    println!("Extracted mint address: {}", token_mint);
                }

                // Log amount if available
                if decoded_data.len() >= 48 {
                    let amount_bytes = &decoded_data[40..48];
                    let amount = u64::from_le_bytes(amount_bytes.try_into().unwrap());
                    token_amount = amount as f64 / 1_000_000.0;
                    println!("Detected transaction amount: {} SOL", 
                        amount as f64 / 1_000_000_000.0);
                }

                // In the case of buy or sell, the bonding curve info is not included in the log messages
                //so we need to get it from the mint account
                let bonding_curve = get_pda(&token_mint, &PUMP_PROGRAM).map_err(|e| e.to_string())?;
                let associated_bonding_curve = get_associated_token_address(&bonding_curve, &token_mint);
                println!("--------------------------------signature: {}", signature);
                println!("token_mint: {}", token_mint);
                println!("token_amount: {:?}", token_amount);
                println!("sol_amount: {:?}", sol_amount);
                println!("bonding_curve: {}", bonding_curve);
                println!("associated_bonding_curve: {}", associated_bonding_curve);
                // Get initial token supply from mint account
                let client = RpcClient::new(std::env::var("RPC_HTTP").unwrap_or_else(|_| "https://mainnet-fra.fountainhead.land/".to_string()));
                if let Ok(mint_pubkey) = Pubkey::from_str(&token_mint) {
                    if let Ok(mint_account) = client.get_account(&mint_pubkey) {
                        if let Ok(mint) = Mint::unpack_from_slice(&mint_account.data) {
                            token_supply = mint.supply;
                        }
                    }
                }
            },
        }
        // Create and return the ParsedTransactionInfo
        Ok(ParsedTransactionInfo {
            slot,
            recent_blockhash, // Use the properly converted hash
            signature,
            transaction_type,
            timestamp,
            mint: token_mint,
            amount: sol_amount,
            token_amount,
            price_per_token,
            volume_change: 0, // We'll calculate this elsewhere
            market_cap: 0.0,  // We'll calculate this elsewhere
            bonding_curve,
            bonding_curve_info: BondingCurveInfo::default(),
            execution_time_ms: 0,
            block_time: None,
            token_name,
            token_symbol,
            sender,
            launcher_sol_balance: 0.0,
        })
    }
}

pub async fn new_token_trader_pumpfun(
    _yellowstone_grpc_http: String,
    _yellowstone_grpc_token: String,
    _yellowstone_ping_interval: u64,
    _yellowstone_reconnect_delay: u64,
    _yellowstone_max_retries: u32,
    _app_state: AppState,
    _swap_config: SwapConfig,
    _blacklist: Blacklist,
    _time_exceed: u64,
    _counter_limit: u64,
    _min_dev_buy: u64,
    _max_dev_buy: u64,
    _telegram_bot_token: String,
    _telegram_chat_id: String,
    _bundle_check: bool,
    _min_last_time: u64,
) -> Result<(), String> {
    // ... function implementation ...
    Ok(())
}

impl Default for BondingCurveInfo {
    fn default() -> Self {
        Self {
            bonding_curve: Pubkey::default(),
            new_virtual_sol_reserve: INITIAL_VIRTUAL_SOL_RESERVES,
            new_virtual_token_reserve: INITIAL_VIRTUAL_TOKEN_RESERVES,
        }
    }
}
