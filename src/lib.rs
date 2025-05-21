//! Solana VNTR Sniper Crate
// Add the recursion limit to handle the TokenListManager
#![recursion_limit = "256"]

pub mod common;
pub mod core;
pub mod dex;
pub mod engine;
pub mod error;
pub mod services;
// pub mod enhanced_token_trader; // Removed - consolidated into engine/enhanced_token_trader.rs
pub mod tests;

pub use engine::monitor::new_token_trader_pumpfun;
pub use engine::enhanced_token_trader::start as start_enhanced_trading_system;
pub use error::ErrorType as Error;

// No duplicate module declaration needed
