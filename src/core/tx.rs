use std::{sync::Arc, time::Duration};
use std::{str::FromStr, env};
use anyhow::Result;
use colored::Colorize;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    signature::Keypair,
    signer::Signer,
    system_instruction, system_transaction,
    transaction::{Transaction, VersionedTransaction},
};
use spl_token::ui_amount_to_amount;

use jito_json_rpc_client::jsonrpc_client::rpc_client::RpcClient as JitoRpcClient;
use tokio::time::Instant;

use crate::common::logger::Logger;
use crate::{
    services::{
        jito::{self, JitoClient},
        nozomi,
        zeroslot::{self, ZeroSlotClient},
    },
};

pub async fn jito_confirm(
    client: &RpcClient,
    keypair: &Keypair,
    version_tx: VersionedTransaction,
    recent_block_hash: &Hash,
    logger: &Logger,
) -> Result<Vec<String>> {
   
      
}

pub async fn new_signed_and_send(
    recent_blockhash: anchor_client::solana_sdk::hash::Hash,
    keypair: &Keypair,
    mut instructions: Vec<Instruction>,
    logger: &Logger,
) -> Result<Vec<String>> {
    let start_time = Instant::now();

    let mut txs = vec![];
    let (tip_account, tip1_account) = jito::get_tip_account()?;

    // jito tip, the upper limit is 0.1
    let tip = jito::get_tip_value().await?;
    let fee = jito::get_priority_fee().await?;
    let tip_lamports = ui_amount_to_amount(tip, spl_token::native_mint::DECIMALS);
    let fee_lamports = ui_amount_to_amount(fee, spl_token::native_mint::DECIMALS);

    let jito_tip_instruction =
        system_instruction::transfer(&keypair.pubkey(), &tip_account, tip_lamports);
    let _jito_tip2_instruction =
        system_instruction::transfer(&keypair.pubkey(), &tip1_account, fee_lamports);

        // ADD Priority fee
        // -------------
        let unit_limit = get_unit_limit();
        let unit_price = get_unit_price();

    let modify_compute_units =
        anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
            unit_limit,
        );
    let add_priority_fee =
        anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
            unit_price,
        );
    instructions.insert(1, modify_compute_units);
    instructions.insert(2, add_priority_fee);
    
    instructions.push(jito_tip_instruction);
    // instructions.push(jito_tip2_instruction);

    // send init tx
    let txn = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &vec![keypair],
        recent_blockhash,
    );

    // let simulate_result = client.simulate_transaction(&txn)?;
    // logger.log("Tx Stimulate".to_string());
    // if let Some(logs) = simulate_result.value.logs {
    //     for log in logs {
    //         logger.log(log.to_string());
    //     }
    // }
    // if let Some(err) = simulate_result.value.err {
    //     return Err(anyhow::anyhow!("{}", err));
    // };

    let jito_client = Arc::new(JitoClient::new(
        format!("{}/api/v1/transactions", *jito::BLOCK_ENGINE_URL).as_str(),
    ));
    let sig = match jito_client.send_transaction(&txn).await {
        Ok(signature) => signature,
        Err(_) => {
            // logger.log(format!("{}", e));
            return Err(anyhow::anyhow!("Bundle status get timeout"
                .red()
                .italic()
                .to_string()));
        }
    };
    txs.push(sig.clone().to_string());
    logger.log(
        format!("[TXN-ELLAPSED(JITO)]: {:?}", start_time.elapsed())
            .yellow()
            .to_string(),
    );

    Ok(txs)
}

pub async fn new_signed_and_send_zeroslot(
    recent_blockhash: anchor_client::solana_sdk::hash::Hash,
    keypair: &Keypair,
    mut instructions: Vec<Instruction>,
    logger: &Logger,
) -> Result<Vec<String>> {
    let start_time = Instant::now();

    let mut txs = vec![];
    let tip_account = zeroslot::get_tip_account()?;

    // zeroslot tip, the upper limit is 0.1
    let tip = zeroslot::get_tip_value().await?;
    let tip_lamports = ui_amount_to_amount(tip, spl_token::native_mint::DECIMALS);

    let zeroslot_tip_instruction =
        system_instruction::transfer(&keypair.pubkey(), &tip_account, tip_lamports);
    instructions.insert(0, zeroslot_tip_instruction);

    // send init tx
    let txn = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &vec![keypair],
        recent_blockhash,
    );

    // let simulate_result = client.simulate_transaction(&txn)?;
    // logger.log("Tx Stimulate".to_string());
    // if let Some(logs) = simulate_result.value.logs {
    //     for log in logs {
    //         logger.log(log.to_string());
    //     }
    // }
    // if let Some(err) = simulate_result.value.err {
    //     return Err(anyhow::anyhow!("{}", err));
    // };

    let zeroslot_client = Arc::new(ZeroSlotClient::new((*zeroslot::ZERO_SLOT_URL).as_str()));
    let sig = match zeroslot_client.send_transaction(&txn).await {
        Ok(signature) => signature,
        Err(_) => {
            return Err(anyhow::anyhow!("send_transaction status get timeout"
                .red()
                .italic()
                .to_string()));
        }
    };
    txs.push(sig.clone().to_string());
    logger.log(
        format!("[TXN-ELLAPSED]: {:?}", start_time.elapsed())
            .yellow()
            .to_string(),
    );

    Ok(txs)
}

// prioritization fee = UNIT_PRICE * UNIT_LIMIT
fn get_unit_price() -> u64 {
    env::var("UNIT_PRICE")
        .ok()
        .and_then(|v| u64::from_str(&v).ok())
        .unwrap_or(20000)
}

fn get_unit_limit() -> u32 {
    env::var("UNIT_LIMIT")
        .ok()
        .and_then(|v| u32::from_str(&v).ok())
        .unwrap_or(200_000)
}

pub async fn new_signed_and_send_nozomi(
    recent_blockhash: anchor_client::solana_sdk::hash::Hash,
    keypair: &Keypair,
    mut instructions: Vec<Instruction>,
    logger: &Logger,
) -> Result<Vec<String>> {
    let start_time = Instant::now();

 

    Ok(txs)
}

pub async fn new_signed_and_send_spam(
    recent_blockhash: anchor_client::solana_sdk::hash::Hash,
    keypair: &std::sync::Arc<Keypair>,
    instructions: Vec<Instruction>,
    logger: &Logger,
) -> Result<Vec<String>> {
    // Assuming keypair is already defined as Arc<Keypair>
    let logger_clone = logger.clone();
    let keypair_clone = Arc::clone(&keypair); // Clone the Arc for the first future

    let logger_clone1 = logger.clone();
    let keypair_clone1 = Arc::clone(&keypair); // Clone the Arc for the second future

    let logger_clone2 = logger.clone();
    let keypair_clone2 = Arc::clone(&keypair); // Clone the Arc for the second future

    // Clone instructions if necessary
    let instructions_clone_for_jito = instructions.clone();
    let instructions_clone_for_nozomi = instructions.clone();
    let instructions_clone_for_zeroslot = instructions.clone();

    // Create the futures for both transaction sending methods
    let jito_future = tokio::task::spawn(async move {
        new_signed_and_send(
            recent_blockhash,
            &keypair_clone,
            instructions_clone_for_jito,
            &logger_clone,
        )
        .await
    });

    let nozomi_future = tokio::task::spawn(async move {
        new_signed_and_send_nozomi(
            recent_blockhash,
            &keypair_clone1,
            instructions_clone_for_nozomi,
            &logger_clone1,
        )
        .await
    });

    let zeroslot_future = tokio::task::spawn(async move {
        new_signed_and_send_zeroslot(
            recent_blockhash,
            &keypair_clone2,
            instructions_clone_for_zeroslot,
            &logger_clone2,
        )
        .await
    });

    // Await both futures
    let results = futures::future::join_all(vec![jito_future, nozomi_future, zeroslot_future]).await;

    let mut successful_results = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for result in results {
        match result {
            Ok(Ok(res)) => successful_results.push(res[0].clone()), // Push if success
            Ok(Err(e)) => errors.push(e.to_string()), // Collect error message
            Err(e) => errors.push(format!("Task failed: {:?}", e)), // Collect task failure
        }
    }

    // If there are any errors, print them
    if !errors.is_empty() {
        for error in &errors {
            eprintln!("{}", error); // Print errors to stderr
        }

        // If no successful results were collected, return an error
        if successful_results.is_empty() {
            return Err(anyhow::anyhow!(format!("All tasks failed with these errors: {:?}", errors)));
        }
    }

    Ok(successful_results)
}
