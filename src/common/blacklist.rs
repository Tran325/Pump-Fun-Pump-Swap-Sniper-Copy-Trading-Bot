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

/// Blacklist of tokens that should not be traded
#[derive(Clone)]
pub struct Blacklist {
    /// Addresses of tokens in the blacklist
    addresses: HashSet<String>,
    /// Path to the blacklist file
    file_path: String,
}

// Implement custom serialization and deserialization
impl Serialize for Blacklist {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // We only need to serialize the addresses, which is what we'll save to disk
        self.addresses.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Blacklist {
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
        })
    }
}

impl Blacklist {
    /// Create a new blacklist from a JSON file
    pub fn new(file_path: &str) -> Result<Self> {
        let path = Path::new(file_path);
        
        // If file doesn't exist, create an empty blacklist
        if !path.exists() {
            return Ok(Self {
                addresses: HashSet::new(),
                file_path: file_path.to_string(),
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
                        format!("Failed to parse blacklist JSON: {}", e),
                    ));
                }
            }
        };
        
        Ok(Self {
            addresses,
            file_path: file_path.to_string(),
        })
    }
    
    /// Create a new blacklist with a specified set of addresses
    pub fn with_addresses(addresses: HashSet<String>, file_path: &str) -> Self {
        Self {
            addresses,
            file_path: file_path.to_string(),
        }
    }
    
    /// Create an empty blacklist
    pub fn empty(file_path: &str) -> Self {
        Self {
            addresses: HashSet::new(),
            file_path: file_path.to_string(),
        }
    }
    
    /// Get the number of addresses in the blacklist
    pub fn len(&self) -> usize {
        self.addresses.len()
    }
    
    /// Check if the blacklist is empty
    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }
    
    /// Check if an address is in the blacklist
    pub fn is_blacklisted(&self, address: &str) -> bool {
        self.addresses.contains(address)
    }
    
    /// Add an address to the blacklist
    pub fn add_address(&mut self, address: &str) -> bool {
        self.addresses.insert(address.to_string())
    }
    
    /// Remove an address from the blacklist
    pub fn remove_address(&mut self, address: &str) -> bool {
        self.addresses.remove(address)
    }
    
    /// Get all addresses in the blacklist
    pub fn get_addresses(&self) -> Vec<String> {
        self.addresses.iter().cloned().collect()
    }
    
    /// Save the blacklist to the file
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.addresses)?;
        fs::write(&self.file_path, json)?;
        Ok(())
    }
}

/// Thread-safe blacklist manager
#[derive(Clone)]
pub struct BlacklistManager {
    blacklist: Arc<Mutex<Blacklist>>,
    save_interval_ms: u64,
    last_save: Arc<std::sync::Mutex<Instant>>,
}

impl BlacklistManager {
    /// Create a new blacklist manager
    pub fn new(blacklist: Blacklist, save_interval_ms: u64) -> Self {
        Self {
            blacklist: Arc::new(Mutex::new(blacklist)),
            save_interval_ms,
            last_save: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }
    
    /// Get a clone of the blacklist
    pub fn get_blacklist_arc(&self) -> Arc<Mutex<Blacklist>> {
        self.blacklist.clone()
    }
    
    /// Check if an address is in the blacklist
    pub async fn is_blacklisted(&self, address: &str) -> bool {
        let blacklist = self.blacklist.lock().await;
        blacklist.is_blacklisted(address)
    }
    
    /// Add an address to the blacklist
    pub async fn add_address(&self, address: &str) -> bool {
        let mut blacklist = self.blacklist.lock().await;
        let result = blacklist.add_address(address);
        self.check_and_save().await;
        result
    }
    
    /// Remove an address from the blacklist
    pub async fn remove_address(&self, address: &str) -> bool {
        let mut blacklist = self.blacklist.lock().await;
        let result = blacklist.remove_address(address);
        self.check_and_save().await;
        result
    }
    
    /// Check if it's time to save and save the blacklist
    pub async fn check_and_save(&self) -> bool {
        let mut last_save = self.last_save.lock().unwrap();
        let elapsed = last_save.elapsed().as_millis() as u64;
        
        if elapsed >= self.save_interval_ms {
            // Time to save
            let blacklist = self.blacklist.lock().await;
            
            match blacklist.save() {
                Ok(_) => {
                    *last_save = Instant::now();
                    true
                },
                Err(e) => {
                    eprintln!("Failed to save blacklist: {}", e);
                    false
                }
            }
        } else {
            false
        }
    }
    
    /// Force save the blacklist
    pub async fn save(&self) -> Result<()> {
        let blacklist = self.blacklist.lock().await;
        let result = blacklist.save();
        
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
    fn test_blacklist_basics() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap().to_string();
        
        let mut blacklist = Blacklist::empty(&temp_path);
        
        // Test add and check
        assert!(blacklist.add_address("token1"));
        assert!(blacklist.is_blacklisted("token1"));
        assert_eq!(blacklist.len(), 1);
        
        // Test remove
        assert!(blacklist.remove_address("token1"));
        assert!(!blacklist.is_blacklisted("token1"));
        assert_eq!(blacklist.len(), 0);
    }
    
    #[tokio::test]
    async fn test_blacklist_manager() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap().to_string();
        
        let blacklist = Blacklist::empty(&temp_path);
        let manager = BlacklistManager::new(blacklist, 5000);
        
        // Add token
        assert!(manager.add_address("token1").await);
        assert!(manager.is_blacklisted("token1").await);
        
        // Save
        assert!(manager.save().await.is_ok());
        
        // Check file contents
        let content = fs::read_to_string(&temp_path).unwrap();
        let parsed: HashSet<String> = serde_json::from_str(&content).unwrap();
        assert!(parsed.contains("token1"));
    }
}
