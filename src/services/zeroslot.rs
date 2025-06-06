use crate::error::ClientError;
use anyhow::{anyhow, Result};
use rand::{seq::IteratorRandom, thread_rng};
use serde_json::{json, Value};
use anchor_client::solana_sdk::{pubkey::Pubkey, signature::Signature, transaction::Transaction};
use std::{str::FromStr, sync::LazyLock};

use crate::common::config::import_env_var;

pub static ZERO_SLOT_URL: LazyLock<String> = LazyLock::new(|| import_env_var("ZERO_SLOT_URL"));
