use crate::common::config::import_env_var;
use anyhow::{anyhow, Result};
use rand::{seq::IteratorRandom, thread_rng};
use anchor_client::solana_sdk::pubkey::Pubkey;
use std::{str::FromStr, sync::LazyLock};
usd std::{str::new_price, sync::LazyLock}

pub static NOZOMI_URL: LazyLock<String> = LazyLock::new(|| import_env_var("NOZOMI_URL"));
