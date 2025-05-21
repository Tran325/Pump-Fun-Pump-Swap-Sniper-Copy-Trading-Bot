use std::fs;
use std::io::{Error, ErrorKind};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;
use serde::{Serialize, Deserialize};
use tokio::sync::Mutex;
use std::sync::Arc;

// Use io::Result only where needed, not as a general import
type Result<T> = std::io::Result<T>;

/// Whitelist with review cycle functionality
///
/// Tracks tokens that are allowed for trading and periodically
/// reviews them based on activity within a review cycle
#[derive(Clone)]
pub struct Whitelist {
    /// Addresses of tokens in the whitelist
    addresses: HashSet<String>,
    /// Path to the whitelist file
    file_path: String,
    /// Time of the last review
    last_review: Instant,
    /// Duration of the review cycle in milliseconds
    review_cycle_ms: u64,
    /// Tokens seen in the current review cycle
    active_tokens: HashSet<String>,
}

impl Whitelist {
    /// Create a new whitelist from a JSON file
    pub fn new(file_path: &str, review_cycle_ms: u64) -> Result<Self> {
        let path = Path::new(file_path);
        
        // If file doesn't exist, create an empty whitelist
        if !path.exists() {
            return Ok(Self {
                addresses: HashSet::new(),
                file_path: file_path.to_string(),
                last_review: Instant::now(),
                review_cycle_ms,
                active_tokens: HashSet::new(),
            });
        }
        
        // Read from file
        let file_content = fs::read_to_string(file_path)?;
        let addresses: HashSet<String> = if file_content.trim().is_empty() {
            HashSet::new()
        } else {
            match serde_json::from_str(&file_content) {
                Ok(addresses) => addresses,
                Err(e) => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        format!("Failed to parse whitelist JSON: {}", e),
                    ));
                }
            }
        };
        
        Ok(Self {
            addresses,
            file_path: file_path.to_string(),
            last_review: Instant::now(),
            review_cycle_ms,
            active_tokens: HashSet::new(),
        })
    }
    
    /// Create a new whitelist with a specified set of addresses
    pub fn with_addresses(addresses: HashSet<String>, file_path: &str, review_cycle_ms: u64) -> Self {
        Self {
            addresses,
            file_path: file_path.to_string(),
            last_review: Instant::now(),
            review_cycle_ms,
            active_tokens: HashSet::new(),
        }
    }
    
    /// Create an empty whitelist
    pub fn empty(file_path: &str, review_cycle_ms: u64) -> Self {
        Self {
            addresses: HashSet::new(),
            file_path: file_path.to_string(),
            last_review: Instant::now(),
            review_cycle_ms,
            active_tokens: HashSet::new(),
        }
    }
    
    /// Get the number of addresses in the whitelist
    pub fn len(&self) -> usize {
        self.addresses.len()
    }
    
    /// Check if the whitelist is empty
    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }
    
    /// Check if an address is in the whitelist
    pub fn is_whitelisted(&self, address: &str) -> bool {
        self.addresses.contains(address)
    }
    
    /// Add an address to the whitelist and mark it as active in the current cycle
    pub fn add_address(&mut self, address: &str) -> bool {
        let is_new = self.addresses.insert(address.to_string());
        self.active_tokens.insert(address.to_string());
        is_new
    }
    
    /// Remove an address from the whitelist
    pub fn remove_address(&mut self, address: &str) -> bool {
        self.addresses.remove(address)
    }
    
    /// Mark an address as active in the current review cycle
    pub fn mark_as_active(&mut self, address: &str) {
        // Only add to active tokens if it's in the whitelist
        if self.is_whitelisted(address) {
            self.active_tokens.insert(address.to_string());
        }
    }
    
    /// Get all addresses in the whitelist
    pub fn get_addresses(&self) -> Vec<String> {
        self.addresses.iter().cloned().collect()
    }
    
    /// Get active addresses in the current review cycle
    pub fn get_active_addresses(&self) -> Vec<String> {
        self.active_tokens.iter().cloned().collect()
    }
    
    /// Check if a review cycle is complete and process it
    pub fn check_review_cycle(&mut self) -> bool {
        let elapsed = self.last_review.elapsed();
        
        if elapsed.as_millis() as u64 >= self.review_cycle_ms {
            // Review cycle completed
            self.process_review_cycle();
            true
        } else {
            false
        }
    }
    
    /// Process the review cycle by removing inactive tokens
    fn process_review_cycle(&mut self) {
        // Keep only tokens that were active in this cycle
        self.addresses.retain(|address| self.active_tokens.contains(address));
        
        // Reset the active tokens for the next cycle
        self.active_tokens.clear();
        
        // Reset the last review time
        self.last_review = Instant::now();
    }
    
    /// Save the whitelist to the file
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.addresses)?;
        fs::write(&self.file_path, json)?;
        Ok(())
    }
}

// Implement custom serialization and deserialization
impl Serialize for Whitelist {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // We only need to serialize the addresses, which is what we'll save to disk
        self.addresses.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Whitelist {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // When deserializing, we only care about the addresses
        let addresses = HashSet::<String>::deserialize(deserializer)?;
        
        // Default values for the other fields - these will be set properly in the new() method
        Ok(Self {
            addresses,
            file_path: String::new(),
            last_review: Instant::now(),
            review_cycle_ms: 0,
            active_tokens: HashSet::new(),
        })
    }
}

/// Thread-safe whitelist manager
#[derive(Clone)]
pub struct WhitelistManager {
    whitelist: Arc<Mutex<Whitelist>>,
    save_interval_ms: u64,
    last_save: Arc<std::sync::Mutex<Instant>>,
}

impl WhitelistManager {
    /// Create a new whitelist manager
    pub fn new(whitelist: Whitelist, save_interval_ms: u64) -> Self {
        Self {
            whitelist: Arc::new(Mutex::new(whitelist)),
            save_interval_ms,
            last_save: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }
    
    /// Get a clone of the whitelist
    pub fn get_whitelist_arc(&self) -> Arc<Mutex<Whitelist>> {
        self.whitelist.clone()
    }
    
    /// Check if an address is in the whitelist
    pub async fn is_whitelisted(&self, address: &str) -> bool {
        let whitelist = self.whitelist.lock().await;
        whitelist.is_whitelisted(address)
    }
    
    /// Mark an address as active in the current review cycle
    pub async fn mark_as_active(&self, address: &str) {
        let mut whitelist = self.whitelist.lock().await;
        whitelist.mark_as_active(address);
    }
    
    /// Add an address to the whitelist
    pub async fn add_address(&self, address: &str) -> bool {
        let mut whitelist = self.whitelist.lock().await;
        let result = whitelist.add_address(address);
        self.check_and_save().await;
        result
    }
    
    /// Remove an address from the whitelist
    pub async fn remove_address(&self, address: &str) -> bool {
        let mut whitelist = self.whitelist.lock().await;
        let result = whitelist.remove_address(address);
        self.check_and_save().await;
        result
    }
    
    /// Check review cycle and save if needed
    pub async fn check_review_cycle(&self) -> bool {
        let mut whitelist = self.whitelist.lock().await;
        let result = whitelist.check_review_cycle();
        
        if result {
            // If review cycle was processed, save the whitelist
            if let Err(e) = whitelist.save() {
                eprintln!("Failed to save whitelist after review cycle: {}", e);
            }
        }
        
        result
    }
    
    /// Check if it's time to save and save the whitelist
    pub async fn check_and_save(&self) -> bool {
        let mut last_save = self.last_save.lock().unwrap();
        let elapsed = last_save.elapsed().as_millis() as u64;
        
        if elapsed >= self.save_interval_ms {
            // Time to save
            let whitelist = self.whitelist.lock().await;
            
            match whitelist.save() {
                Ok(_) => {
                    *last_save = Instant::now();
                    true
                },
                Err(e) => {
                    eprintln!("Failed to save whitelist: {}", e);
                    false
                }
            }
        } else {
            false
        }
    }
    
    /// Force save the whitelist
    pub async fn save(&self) -> Result<()> {
        let whitelist = self.whitelist.lock().await;
        let result = whitelist.save();
        
        if result.is_ok() {
            let mut last_save = self.last_save.lock().unwrap();
            *last_save = Instant::now();
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    
    #[test]
    fn test_whitelist_basics() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap().to_string();
        
        let mut whitelist = Whitelist::empty(&temp_path, 1000);
        
        // Test add and check
        assert!(whitelist.add_address("token1"));
        assert!(whitelist.is_whitelisted("token1"));
        assert_eq!(whitelist.len(), 1);
        
        // Test mark as active
        whitelist.mark_as_active("token1");
        assert!(whitelist.active_tokens.contains("token1"));
        
        // Test review cycle
        assert!(!whitelist.check_review_cycle()); // Not enough time has passed
        
        // Force review cycle processing
        whitelist.last_review = Instant::now() - Duration::from_millis(2000);
        assert!(whitelist.check_review_cycle());
        
        // After review, token1 should be removed since we cleared active_tokens
        assert!(!whitelist.is_whitelisted("token1"));
        assert_eq!(whitelist.len(), 0);
    }
    
    #[tokio::test]
    async fn test_whitelist_manager() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap().to_string();
        
        let whitelist = Whitelist::empty(&temp_path, 1000);
        let manager = WhitelistManager::new(whitelist, 5000);
        
        // Add token
        assert!(manager.add_address("token1").await);
        assert!(manager.is_whitelisted("token1").await);
        
        // Mark as active
        manager.mark_as_active("token1").await;
        
        // Save
        assert!(manager.save().await.is_ok());
        
        // Check file contents
        let content = fs::read_to_string(&temp_path).unwrap();
        let parsed: HashSet<String> = serde_json::from_str(&content).unwrap();
        assert!(parsed.contains("token1"));
    }
} 