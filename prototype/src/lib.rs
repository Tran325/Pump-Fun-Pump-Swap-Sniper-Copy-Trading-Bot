use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use regex::Regex;
use lazy_static::lazy_static;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenBalance {
    pub account_index: usize,
    pub mint: String,
    pub ui_token_amount: Option<UiTokenAmount>,
    pub owner: String,
    pub program_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UiTokenAmount {
    pub ui_amount: f64,
    pub decimals: u8,
    pub amount: String,
    pub ui_amount_string: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PumpTransaction {
    pub transaction_type: TransactionType,
    pub signature: String,
    pub slot: u64,
    pub timestamp: Option<String>,
    pub success: bool,
    pub fee: u64,
    pub log_messages: Vec<String>,
    pub token_balances: TokenBalances,
    pub key_accounts: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenBalances {
    pub pre_balances: Vec<TokenBalance>,
    pub post_balances: Vec<TokenBalance>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TransactionType {
    Buy,
    Sell,
    Mint,
    Unknown,
}

/// Parses a transaction log file and returns a PumpTransaction
pub fn parse_transaction_log(file_path: &Path) -> Result<PumpTransaction> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    
    lazy_static! {
        static ref SIGNATURE_RE: Regex = Regex::new(r"Transaction Signature: \[(.*?)\]").unwrap();
        static ref SLOT_RE: Regex = Regex::new(r"slot: (\d+)").unwrap();
        static ref SUCCESS_RE: Regex = Regex::new(r"Transaction Success: (\w+)").unwrap();
        static ref FEE_RE: Regex = Regex::new(r"fee: (\d+)").unwrap();
        static ref LOG_START_RE: Regex = Regex::new(r"==== BEGIN TRANSACTION LOG ====").unwrap();
        static ref LOG_END_RE: Regex = Regex::new(r"==== END TRANSACTION LOG ====").unwrap();
        static ref LOG_LINE_RE: Regex = Regex::new(r"LOG\[\d+\]: (.*)").unwrap();
        static ref INSTRUCTION_RE: Regex = Regex::new(r"Program log: Instruction: (\w+)").unwrap();
    }
    
    let mut tx = PumpTransaction {
        transaction_type: TransactionType::Unknown,
        signature: String::new(),
        slot: 0,
        timestamp: None,
        success: false,
        fee: 0,
        log_messages: Vec::new(),
        token_balances: TokenBalances {
            pre_balances: Vec::new(),
            post_balances: Vec::new(),
        },
        key_accounts: HashMap::new(),
    };
    
    let mut in_log_section = false;
    let mut content = String::new();
    
    for line in reader.lines() {
        let line = line?;
        content.push_str(&line);
        content.push('\n');
        
        // Extract signature
        if let Some(captures) = SIGNATURE_RE.captures(&line) {
            if let Some(signature_match) = captures.get(1) {
                tx.signature = signature_match.as_str().to_string();
            }
        }
        
        // Extract slot
        if let Some(captures) = SLOT_RE.captures(&line) {
            if let Some(slot_match) = captures.get(1) {
                tx.slot = slot_match.as_str().parse()?;
            }
        }
        
        // Extract success
        if let Some(captures) = SUCCESS_RE.captures(&line) {
            if let Some(success_match) = captures.get(1) {
                tx.success = success_match.as_str() == "true";
            }
        }
        
        // Extract fee
        if let Some(captures) = FEE_RE.captures(&line) {
            if let Some(fee_match) = captures.get(1) {
                tx.fee = fee_match.as_str().parse()?;
            }
        }
        
        // Handle log section
        if LOG_START_RE.is_match(&line) {
            in_log_section = true;
            continue;
        }
        
        if LOG_END_RE.is_match(&line) {
            in_log_section = false;
            continue;
        }
        
        if in_log_section {
            if let Some(captures) = LOG_LINE_RE.captures(&line) {
                if let Some(log_match) = captures.get(1) {
                    let log_message = log_match.as_str().to_string();
                    tx.log_messages.push(log_message.clone());
                    
                    // Determine transaction type from instruction
                    if let Some(captures) = INSTRUCTION_RE.captures(&log_message) {
                        if let Some(instruction_match) = captures.get(1) {
                            let instruction = instruction_match.as_str();
                            match instruction {
                                "Buy" => tx.transaction_type = TransactionType::Buy,
                                "Sell" => tx.transaction_type = TransactionType::Sell,
                                "Create" | "MintTo" => {
                                    if tx.transaction_type != TransactionType::Buy && tx.transaction_type != TransactionType::Sell {
                                        tx.transaction_type = TransactionType::Mint;
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
    
    // Process token balances from the content
    extract_token_balances(&content, &mut tx)?;
    
    // Extract key accounts
    extract_key_accounts(&content, &mut tx)?;
    
    Ok(tx)
}

fn extract_token_balances(content: &str, tx: &mut PumpTransaction) -> Result<()> {
    lazy_static! {
        static ref PRE_BALANCE_RE: Regex = Regex::new(r"pre_token_balances: \[(.*?)\]").unwrap();
        static ref POST_BALANCE_RE: Regex = Regex::new(r"post_token_balances: \[(.*?)\]").unwrap();
        static ref TOKEN_BALANCE_LINE_RE: Regex = Regex::new(r"TokenBalance \{ account_index: (\d+), mint: \"([^\"]+)\", ui_token_amount: Some\(UiTokenAmount \{ ui_amount: ([^,]+), decimals: (\d+), amount: \"([^\"]+)\", ui_amount_string: \"([^\"]+)\" \}\), owner: \"([^\"]+)\", program_id: \"([^\"]+)\" \}").unwrap();
    }
    
    // Extract pre_token_balances
    if let Some(captures) = PRE_BALANCE_RE.captures(content) {
        if let Some(balances_match) = captures.get(1) {
            let balances_str = balances_match.as_str();
            for captures in TOKEN_BALANCE_LINE_RE.captures_iter(balances_str) {
                let account_index = captures[1].parse::<usize>()?;
                let mint = captures[2].to_string();
                let ui_amount = captures[3].parse::<f64>()?;
                let decimals = captures[4].parse::<u8>()?;
                let amount = captures[5].to_string();
                let ui_amount_string = captures[6].to_string();
                let owner = captures[7].to_string();
                let program_id = captures[8].to_string();
                
                tx.token_balances.pre_balances.push(TokenBalance {
                    account_index,
                    mint,
                    ui_token_amount: Some(UiTokenAmount {
                        ui_amount,
                        decimals,
                        amount,
                        ui_amount_string,
                    }),
                    owner,
                    program_id,
                });
            }
        }
    }
    
    // Extract post_token_balances
    if let Some(captures) = POST_BALANCE_RE.captures(content) {
        if let Some(balances_match) = captures.get(1) {
            let balances_str = balances_match.as_str();
            for captures in TOKEN_BALANCE_LINE_RE.captures_iter(balances_str) {
                let account_index = captures[1].parse::<usize>()?;
                let mint = captures[2].to_string();
                let ui_amount = captures[3].parse::<f64>()?;
                let decimals = captures[4].parse::<u8>()?;
                let amount = captures[5].to_string();
                let ui_amount_string = captures[6].to_string();
                let owner = captures[7].to_string();
                let program_id = captures[8].to_string();
                
                tx.token_balances.post_balances.push(TokenBalance {
                    account_index,
                    mint,
                    ui_token_amount: Some(UiTokenAmount {
                        ui_amount,
                        decimals,
                        amount,
                        ui_amount_string,
                    }),
                    owner,
                    program_id,
                });
            }
        }
    }
    
    Ok(())
}

fn extract_key_accounts(content: &str, tx: &mut PumpTransaction) -> Result<()> {
    lazy_static! {
        static ref ACCOUNT_KEYS_RE: Regex = Regex::new(r"Key\[(\d+)\]: \[(.*?)\]").unwrap();
    }
    
    for captures in ACCOUNT_KEYS_RE.captures_iter(content) {
        let index = captures[1].parse::<usize>()?;
        let key_data = captures[2].to_string();
        tx.key_accounts.insert(format!("key_{}", index), key_data);
    }
    
    Ok(())
}

/// Calculates the token amount difference between pre and post balances
pub fn calculate_token_amount_change(tx: &PumpTransaction) -> HashMap<String, f64> {
    let mut changes = HashMap::new();
    
    // Create a map of mint -> pre_balance
    let mut pre_balances = HashMap::new();
    for balance in &tx.token_balances.pre_balances {
        if let Some(token_amount) = &balance.ui_token_amount {
            pre_balances.insert(balance.mint.clone(), (token_amount.ui_amount, balance.owner.clone()));
        }
    }
    
    // Calculate differences with post_balances
    for balance in &tx.token_balances.post_balances {
        if let Some(token_amount) = &balance.ui_token_amount {
            let key = format!("{}:{}", balance.mint, balance.owner);
            let pre_amount = pre_balances
                .get(&balance.mint)
                .map(|(amount, owner)| {
                    if *owner == balance.owner { *amount } else { 0.0 }
                })
                .unwrap_or(0.0);
            
            let change = token_amount.ui_amount - pre_amount;
            changes.insert(key, change);
        }
    }
    
    changes
}

/// Generate a summary of the transaction
pub fn generate_transaction_summary(tx: &PumpTransaction) -> String {
    let tx_type = match tx.transaction_type {
        TransactionType::Buy => "Buy",
        TransactionType::Sell => "Sell",
        TransactionType::Mint => "Mint",
        TransactionType::Unknown => "Unknown",
    };
    
    let changes = calculate_token_amount_change(tx);
    
    let mut summary = format!("Transaction Type: {}\n", tx_type);
    summary.push_str(&format!("Signature: {}\n", tx.signature));
    summary.push_str(&format!("Slot: {}\n", tx.slot));
    summary.push_str(&format!("Success: {}\n", tx.success));
    summary.push_str(&format!("Fee: {} lamports\n", tx.fee));
    
    summary.push_str("\nToken Balance Changes:\n");
    for (key, change) in changes {
        let parts: Vec<&str> = key.split(':').collect();
        if parts.len() == 2 {
            let (mint, owner) = (parts[0], parts[1]);
            let change_str = if change > 0.0 {
                format!("+{:.6}", change)
            } else {
                format!("{:.6}", change)
            };
            summary.push_str(&format!("  Mint: {}\n  Owner: {}\n  Change: {}\n\n", mint, owner, change_str));
        }
    }
    
    summary
} 