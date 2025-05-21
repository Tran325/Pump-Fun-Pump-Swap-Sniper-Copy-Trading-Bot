use anchor_client::solana_sdk::{pubkey::Pubkey, signature::Keypair};
use spl_token::state::{Account, Mint};
use solana_program_pack::Pack;
use spl_token_client::{
    client::{ProgramClient, ProgramRpcClient, ProgramRpcClientSendTransaction},
    token::{Token, TokenError, TokenResult},
};
use std::sync::Arc;

pub fn get_associated_token_address(
    client: Arc<anchor_client::solana_client::nonblocking::rpc_client::RpcClient>,
    keypair: Arc<Keypair>,
    address: &Pubkey,
    owner: &Pubkey,
) -> Pubkey {
    let token_client = Token::new(
        Arc::new(ProgramRpcClient::new(
            client.clone(),
            ProgramRpcClientSendTransaction,
        )),
        &spl_token::ID,
        address,
        None,
        Arc::new(Keypair::from_bytes(&keypair.to_bytes()).expect("failed to copy keypair")),
    );
    token_client.get_associated_token_address(owner)
}

pub async fn get_account_info(
    client: Arc<anchor_client::solana_client::nonblocking::rpc_client::RpcClient>,
    address: Pubkey,
    account: Pubkey,
) -> TokenResult<Account> {
    let program_client = Arc::new(ProgramRpcClient::new(
        client.clone(),
        ProgramRpcClientSendTransaction,
    ));
    let account_data = program_client
        .get_account(account)
        .await
        .map_err(TokenError::Client)?
        .ok_or(TokenError::AccountNotFound)
        .inspect_err(|_err| {
            // logger.log(format!(
            //     "get_account_info: {} {}: mint {}",
            //     account, err, address
            // ));
        })?;

    if account_data.owner != spl_token::ID {
        return Err(TokenError::AccountInvalidOwner);
    }
    
    let account_state = Account::unpack(&account_data.data)?;
    if account_state.mint != address {
        return Err(TokenError::AccountInvalidMint);
    }

    Ok(account_state)
}

pub async fn get_mint_info(
    client: Arc<anchor_client::solana_client::nonblocking::rpc_client::RpcClient>,
    _keypair: Arc<Keypair>,
    address: Pubkey,
) -> TokenResult<Mint> {
    let program_client = Arc::new(ProgramRpcClient::new(
        client.clone(),
        ProgramRpcClientSendTransaction,
    ));
    let account = program_client
        .get_account(address)
        .await
        .map_err(TokenError::Client)?
        .ok_or(TokenError::AccountNotFound)
        .inspect_err(|err| println!("{} {}: mint {}", address, err, address))?;

    if account.owner != spl_token::ID {
        return Err(TokenError::AccountInvalidOwner);
    }

    let mint_state = Mint::unpack(&account.data)
        .map_err(|_| TokenError::AccountInvalidMint)?;
    let decimals: Option<u8> = None;
    if let Some(decimals) = decimals {
        if decimals != mint_state.decimals {
            return Err(TokenError::InvalidDecimals);
        }
    }

    Ok(mint_state)
}
