use pumpfun_analyzer::{parse_transaction_log, generate_transaction_summary, PumpTransaction, TransactionType};
use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use colored::*;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

#[derive(Parser)]
#[command(name = "pumpfun-cli")]
#[command(about = "CLI for analyzing PumpFun transactions", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a single transaction log file
    Parse {
        /// Path to the log file
        #[arg(short, long)]
        file: PathBuf,
        
        /// Output path for the JSON result
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    
    /// Analyze multiple transaction logs
    Analyze {
        /// Directory containing log files
        #[arg(short, long)]
        dir: PathBuf,
        
        /// Filter by transaction type (buy, sell, mint)
        #[arg(short, long)]
        transaction_type: Option<String>,
        
        /// Output directory for analysis results
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn process_single_file(file_path: &Path, output_path: Option<&Path>) -> Result<()> {
    println!("{} {}", "Parsing".green(), file_path.display());
    
    let tx = parse_transaction_log(file_path)?;
    let summary = generate_transaction_summary(&tx);
    
    println!("\n{}", summary);
    
    if let Some(output) = output_path {
        let json = serde_json::to_string_pretty(&tx)?;
        let mut file = File::create(output)?;
        file.write_all(json.as_bytes())?;
        println!("{} {}", "Results saved to".green(), output.display());
    }
    
    Ok(())
}

fn analyze_directory(dir_path: &Path, tx_type_filter: Option<&str>, output_dir: Option<&Path>) -> Result<()> {
    if !dir_path.exists() || !dir_path.is_dir() {
        return Err(anyhow!("Invalid directory path: {}", dir_path.display()));
    }
    
    println!("{} {}", "Analyzing files in".green(), dir_path.display());
    
    let mut transactions = Vec::new();
    let mut type_counts = HashMap::new();
    // let mut total_fees = 0u64;
    let mut tx_type_filter = Hash
    
    // Process files in directory
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "log") {
            match parse_transaction_log(&path) {
                Ok(tx) => {
                    // Apply transaction type filter if specified
                    let tx_type_str = match tx.transaction_type {
                        TransactionType::Buy => "buy",
                        TransactionType::Sell => "sell",
                        TransactionType::Mint => "mint",
                        TransactionType::Unknown => "unknown",
                    };
                    
                    if let Some(filter) = tx_type_filter {
                        if filter.to_lowercase() != tx_type_str {
                            continue;
                        }
                    }
                    
                    // Collect statistics
                    *type_counts.entry(tx_type_str.to_string()).or_insert(0) += 1;
                    total_fees += tx.fee;
                    
                    // Add to collections
                    transactions.push(tx);
                }
                Err(e) => {
                    eprintln!("{} {} - {}", "Error parsing".red(), path.display(), e);
                }
            }
        }
    }
    
    // Display summary
    println!("\n{}", "=== Analysis Summary ===".cyan().bold());
    println!("Total transactions: {}", transactions.len());
    println!("Transaction types:");
    for (tx_type, count) in &type_counts {
        println!("  {}: {}", tx_type, count);
    }
    println!("Total fees: {} lamports", total_fees);
    
    // Save results if output directory specified
    if let Some(output) = output_dir {
        if !output.exists() {
            fs::create_dir_all(output)?;
        }
        
        // Save all transactions to a single JSON file
        let all_txs_path = output.join("all_transactions.json");
        let json = serde_json::to_string_pretty(&transactions)?;
        let mut file = File::create(all_txs_path)?;
        file.write_all(json.as_bytes())?;
        
        // Save summary to a text file
        let summary_path = output.join("summary.txt");
        let mut summary_file = File::create(summary_path)?;
        writeln!(summary_file, "=== Analysis Summary ===")?;
        writeln!(summary_file, "Total transactions: {}", transactions.len())?;
        writeln!(summary_file, "Transaction types:")?;
        for (tx_type, count) in &type_counts {
            writeln!(summary_file, "  {}: {}", tx_type, count)?;
        }
        writeln!(summary_file, "Total fees: {} lamports", total_fees)?;
        
        println!("{} {}", "Results saved to".green(), output.display());
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Parse { file, output } => {
            process_single_file(&file, output.as_deref())?;
        }
        Commands::Analyze { dir, transaction_type, output } => {
            analyze_directory(&dir, transaction_type.as_deref(), output.as_deref())?;
        }
    }
    
    Ok(())
} 