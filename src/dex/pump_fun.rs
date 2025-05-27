use std::{str::FromStr, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use borsh::from_slice;
use borsh_derive::{BorshDeserialize, BorshSerialize};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use anchor_client::solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account,
};
use spl_token::{amount_to_ui_amount, ui_amount_to_amount};
use spl_token_client::token::TokenError;
use tokio::time::Instant;

use crate::{
    common::{config::SwapConfig, logger::Logger},
    core::token,
    engine::{monitor::BondingCurveInfo, swap::{SwapDirection, SwapInType}},
};

pub const TEN_THOUSAND: u64 = 10000;
pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const RENT_PROGRAM: &str = "SysvarRent111111111111111111111111111111111";
pub const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const PUMP_GLOBAL: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
pub const PUMP_FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
pub const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
// pub const PUMP_FUN_MINT_AUTHORITY: &str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM";
pub const PUMP_ACCOUNT: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
pub const PUMP_BUY_METHOD: u64 = 16927863322537952870;
pub const PUMP_SELL_METHOD: u64 = 12502976635542562355;
pub const PUMP_FUN_CREATE_IX_DISCRIMINATOR: &[u8] = &[24, 30, 200, 40, 5, 28, 7, 119];
pub const INITIAL_VIRTUAL_SOL_RESERVES: u64 = 30_000_000_000;
pub const INITIAL_VIRTUAL_TOKEN_RESERVES: u64 = 1_073_000_000_000_000;
pub const TOKEN_TOTAL_SUPPLY: u64 = 1_000_000_000_000_000;

#[derive(Clone)]
pub struct Pump {
    pub rpc_nonblocking_client: Arc<anchor_client::solana_client::nonblocking::rpc_client::RpcClient>,
    pub keypair: Arc<Keypair>,
    pub rpc_client: Option<Arc<anchor_client::solana_client::rpc_client::RpcClient>>,
}

impl Pump {
    pub fn new(
        rpc_nonblocking_client: Arc<anchor_client::solana_client::nonblocking::rpc_client::RpcClient>,
        rpc_client: Arc<anchor_client::solana_client::rpc_client::RpcClient>,
        keypair: Arc<Keypair>,
    ) -> Self {
        Self {
            rpc_nonblocking_client,
            keypair,
            rpc_client: Some(rpc_client),
        }
    }

  

pub async fn get_bonding_curve_account(
    rpc_client: Arc<anchor_client::solana_client::rpc_client::RpcClient>,
    mint: Pubkey,
    program_id: Pubkey,
) -> Result<(Pubkey, Pubkey, BondingCurveReserves)> {
    let bonding_curve = get_pda(&mint, &program_id)?;
    let associated_bonding_curve = get_associated_token_address(&bonding_curve, &mint);
    let start_time = Instant::now();
    // println!("Start: {:?}", start_time.elapsed());

    let max_retries = 30;
    let time_exceed = 300;
    let timeout = Duration::from_millis(time_exceed);
    let mut retry_count = 0;
    let bonding_curve_data = loop {
        match rpc_client.get_account_data(&bonding_curve) {
            Ok(data) => {
                // println!("Done: {:?}", start_time.elapsed());
                break data;
            }
            Err(err) => {
                retry_count += 1;
                if retry_count > max_retries {
                    return Err(anyhow!(
                        "Failed to get bonding curve account data after {} retries: {}",
                        max_retries,
                        err
                    ));
                }
                if start_time.elapsed() > timeout {
                    return Err(anyhow!(
                        "Failed to get bonding curve account data after {:?} timeout: {}",
                        timeout,
                        err
                    ));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
                // println!("Retry {}: {:?}", retry_count, start_time.elapsed());
            }
        }
    };

    let bonding_curve_account =
        from_slice::<BondingCurveAccount>(&bonding_curve_data).map_err(|e| {
            anyhow!(
                "Failed to deserialize bonding curve account: {}",
                e.to_string()
            )
        })?;
    let bonding_curve_reserves = BondingCurveReserves 
        { 
            virtual_token_reserves: bonding_curve_account.virtual_token_reserves, 
            virtual_sol_reserves: bonding_curve_account.virtual_sol_reserves 
        };
    Ok((
        bonding_curve,
        associated_bonding_curve,
        bonding_curve_reserves,
    ))
}

pub fn get_pda(mint: &Pubkey, program_id: &Pubkey) -> Result<Pubkey> {
    let seeds = [b"bonding-curve".as_ref(), mint.as_ref()];
    let (bonding_curve, _bump) = Pubkey::find_program_address(&seeds, program_id);
    Ok(bonding_curve)
}

// https://frontend-api.pump.fun/coins/8zSLdDzM1XsqnfrHmHvA9ir6pvYDjs8UXz6B2Tydd6b2
// pub async fn get_pump_info(
//     rpc_client: Arc<solana_client::rpc_client::RpcClient>,
//     mint: str,
// ) -> Result<PumpInfo> {
//     let mint = Pubkey::from_str(&mint)?;
//     let program_id = Pubkey::from_str(PUMP_PROGRAM)?;
//     let (bonding_curve, associated_bonding_curve, bonding_curve_account) =
//         get_bonding_curve_account(rpc_client, &mint, &program_id).await?;

//     let pump_info = PumpInfo {
//         mint: mint.to_string(),
//         bonding_curve: bonding_curve.to_string(),
//         associated_bonding_curve: associated_bonding_curve.to_string(),
//         raydium_pool: None,
//         raydium_info: None,
//         complete: bonding_curve_account.complete,
//         virtual_sol_reserves: bonding_curve_account.virtual_sol_reserves,
//         virtual_token_reserves: bonding_curve_account.virtual_token_reserves,
//         total_supply: bonding_curve_account.token_total_supply,
//     };
//     Ok(pump_info)
// }

/// Token pool information structure
#[derive(Debug, Clone)]
pub struct PoolInfo {
    /// Token mint address
    pub token_mint: String,
    /// Current liquidity in SOL
    pub liquidity: f64,
    /// Current token price in SOL
    pub price: f64,
    /// Virtual SOL reserves (from bonding curve)
    pub virtual_sol_reserves: f64,
    /// Virtual token reserves (from bonding curve)
    pub virtual_token_reserves: f64,
}
