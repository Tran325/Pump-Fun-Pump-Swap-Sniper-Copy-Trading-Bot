use clap::ValueEnum;
use serde::Deserialize;
use std::str::FromStr;
use anyhow::{Error, Result};

#[derive(ValueEnum, Debug, Clone, Deserialize, PartialEq)]
pub enum SwapDirection {
    #[serde(rename = "buy")]
    Buy,
    #[serde(rename = "sell")]
    Sell,
}
impl From<SwapDirection> for u8 {
    fn from(value: SwapDirection) -> Self {
        match value {
            SwapDirection::Buy => 0,
            SwapDirection::Sell => 1,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Deserialize)]
pub enum SwapInType {
    /// Quantity
    #[serde(rename = "qty")]
    Qty,
    /// Percentage
    #[serde(rename = "pct")]
    Pct,
}

impl FromStr for SwapDirection {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "buy" => Ok(SwapDirection::Buy),
            "sell" => Ok(SwapDirection::Sell),
            _ => Err(anyhow::anyhow!("Invalid swap direction: {}", s)),
        }
    }
}

impl FromStr for SwapInType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "qty" => Ok(SwapInType::Qty),
            "pct" => Ok(SwapInType::Pct),
            _ => Err(anyhow::anyhow!("Invalid swap in type: {}", s)),
        }
    }
}
