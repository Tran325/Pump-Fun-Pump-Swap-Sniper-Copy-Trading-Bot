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
