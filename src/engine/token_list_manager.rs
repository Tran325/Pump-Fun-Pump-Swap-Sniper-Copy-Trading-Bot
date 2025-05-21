use std::sync::Arc;
use std::time::Instant;
use tokio::time::Duration;
use tokio::sync::Mutex;

use crate::common::{
    logger::Logger,
    blacklist::{Blacklist, BlacklistManager},
    whitelist::{Whitelist, WhitelistManager},
};

/// Manager for handling token lists (whitelist/blacklist) with review cycles
#[allow(dead_code)]
pub struct TokenListManager {
    /// Whitelist manager
    whitelist_manager: WhitelistManager,
    /// Blacklist manager
    blacklist_manager: BlacklistManager,
    /// Logger
    logger: Logger,
    /// Last save timestamp
    last_save: Arc<std::sync::Mutex<Instant>>,
    /// Save interval in milliseconds
    save_interval_ms: u64,
    /// Tokens seen in the current review cycle
    active_tokens_in_cycle: Arc<Mutex<Vec<String>>>,
}

impl TokenListManager {
    /// Create a new token list manager
    pub fn new(
        whitelist_path: &str,
        blacklist_path: &str, 
        review_cycle_ms: u64,
        save_interval_ms: u64,
        logger: Logger
    ) -> Result<Self, std::io::Error> {
        // Initialize whitelist
        let whitelist = Whitelist::new(whitelist_path, review_cycle_ms)?;
        let whitelist_manager = WhitelistManager::new(whitelist.clone(), save_interval_ms);
        
        // Initialize blacklist
        let blacklist = Blacklist::new(blacklist_path)?;
        let blacklist_manager = BlacklistManager::new(blacklist.clone(), save_interval_ms);
        
        logger.log(format!(
            "TokenListManager initialized - Whitelist: {} tokens, Blacklist: {} tokens", 
            whitelist.len(),
            blacklist.len()
        ));
        
        Ok(Self {
            whitelist_manager,
            blacklist_manager,
            logger,
            last_save: Arc::new(std::sync::Mutex::new(Instant::now())),
            save_interval_ms,
            active_tokens_in_cycle: Arc::new(Mutex::new(Vec::new())),
        })
    }
    
    /// Process a token, checking blacklist/whitelist and marking as active in the current cycle
    pub async fn process_token(&self, token_mint: &str) -> TokenListStatus {
        // First check blacklist
        if self.blacklist_manager.is_blacklisted(token_mint).await {
            return TokenListStatus::Blacklisted;
        }
        
        // Then handle whitelist
        let is_whitelisted = self.whitelist_manager.is_whitelisted(token_mint).await;
        // Add to active tokens in this cycle
        let mut active_tokens = self.active_tokens_in_cycle.lock().await;
        if !active_tokens.contains(&token_mint.to_string()) {
            active_tokens.push(token_mint.to_string());
        }
        
        // Mark as active in whitelist if it's there
        if is_whitelisted {
            self.whitelist_manager.mark_as_active(token_mint).await;
            TokenListStatus::Whitelisted
        } else {
            TokenListStatus::NotListed
        }
    }
    
    /// Check if it's time for a review cycle and process it
    pub async fn check_review_cycle(&self) -> bool {
        let review_processed = self.whitelist_manager.check_review_cycle().await;
        
        if review_processed {
            self.logger.log("Review cycle completed - Updated whitelist with active tokens only".to_string());
            
            // Reset active tokens for the next cycle
            let mut active_tokens = self.active_tokens_in_cycle.lock().await;
            active_tokens.clear();
            
            // Force save both lists
            let _ = self.whitelist_manager.save().await;
            let _ = self.blacklist_manager.save().await;
        }
        
        review_processed
    }
    
    /// Check if a token is blacklisted
    pub async fn is_blacklisted(&self, token_mint: &str) -> bool {
        self.blacklist_manager.is_blacklisted(token_mint).await
    }
    
    /// Check if a token is whitelisted
    pub async fn is_whitelisted(&self, token_mint: &str) -> bool {
        self.whitelist_manager.is_whitelisted(token_mint).await
    }
    
    /// Add a token to the blacklist
    pub async fn add_to_blacklist(&self, token_mint: &str) -> bool {
        self.blacklist_manager.add_address(token_mint).await
    }
    
    /// Add a token to the whitelist
    pub async fn add_to_whitelist(&self, token_mint: &str) -> bool {
        self.whitelist_manager.add_address(token_mint).await
    }
    
    /// Get active tokens in the current cycle
    pub async fn get_active_tokens(&self) -> Vec<String> {
        let active_tokens = self.active_tokens_in_cycle.lock().await;
        active_tokens.clone()
    }
    
    /// Start background task for periodic saving and review cycle checking
    pub fn start_background_tasks(&self) {
        let whitelist_manager = self.whitelist_manager.clone();
        let blacklist_manager = self.blacklist_manager.clone();
        let logger = self.logger.clone();
        let save_interval_ms = self.save_interval_ms;
        
        tokio::spawn(async move {
            let mut save_interval = tokio::time::interval(Duration::from_millis(save_interval_ms));
            
            loop {
                save_interval.tick().await;
                
                // Check and process review cycle
                if whitelist_manager.check_review_cycle().await {
                    logger.log("Review cycle completed - Updated whitelist with active tokens only".to_string());
                }
                
                // Save lists
                if let Err(e) = whitelist_manager.save().await {
                    logger.log(format!("Error saving whitelist: {}", e));
                }
                
                if let Err(e) = blacklist_manager.save().await {
                    logger.log(format!("Error saving blacklist: {}", e));
                }
            }
        });
    }
    
    /// Mark a token as active in the current review cycle
    pub async fn mark_as_active(&self, token_mint: &str) {
        // Mark as active in the whitelist
        self.whitelist_manager.mark_as_active(token_mint).await;
        
        // Add to active tokens in this cycle
        let mut active_tokens = self.active_tokens_in_cycle.lock().await;
        if !active_tokens.contains(&token_mint.to_string()) {
            active_tokens.push(token_mint.to_string());
        }
    }
}

/// Status of a token in relation to the whitelist/blacklist
#[derive(Debug, Clone, PartialEq)]
pub enum TokenListStatus {
    /// Token is in the whitelist
    Whitelisted,
    /// Token is in the blacklist
    Blacklisted,
    /// Token is not in either list
    NotListed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use colored::Colorize;
    
    #[tokio::test]
    async fn test_token_list_manager() {
        let whitelist_file = NamedTempFile::new().unwrap();
        let blacklist_file = NamedTempFile::new().unwrap();
        
        let whitelist_path = whitelist_file.path().to_str().unwrap();
        let blacklist_path = blacklist_file.path().to_str().unwrap();
        
        let logger = Logger::new("[TEST] => ".blue().to_string());
        
        let manager = TokenListManager::new(
            whitelist_path,
            blacklist_path,
            1000, // 1 second review cycle
            5000, // 5 second save interval
            logger
        ).unwrap();
        
        // Test blacklist
        assert!(manager.add_to_blacklist("blacktoken").await);
        assert!(manager.is_blacklisted("blacktoken").await);
        
        // Test whitelist
        assert!(manager.add_to_whitelist("whitetoken").await);
        assert!(manager.is_whitelisted("whitetoken").await);
        
        // Test process token
        assert_eq!(manager.process_token("blacktoken").await, TokenListStatus::Blacklisted);
        assert_eq!(manager.process_token("whitetoken").await, TokenListStatus::Whitelisted);
        assert_eq!(manager.process_token("unlisted").await, TokenListStatus::NotListed);
        
        // Verify active tokens
        let active_tokens = manager.get_active_tokens().await;
        assert!(active_tokens.contains(&"blacktoken".to_string()));
        assert!(active_tokens.contains(&"whitetoken".to_string()));
        assert!(active_tokens.contains(&"unlisted".to_string()));
    }
} 