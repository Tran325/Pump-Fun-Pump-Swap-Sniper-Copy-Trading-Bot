#!/bin/bash

# Check if the errors are still in the compiled output
echo "Checking for E0308 mismatched types errors..."
cargo check --lib 2>&1 | grep -A 2 "error\[E0308\].*expected .Arc<SwapConfig>., found .Arc<Arc<SwapConfig>>."
cargo check --lib 2>&1 | grep -A 2 "error\[E0308\].*expected .&Arc<TokenTracker>., found .&Arc<Mutex<TokenTracker>>."

echo "Checking for E0599 no method named lock errors..."
cargo check --lib 2>&1 | grep -A 2 "error\[E0599\].*no method named .lock. found for struct"

echo "If no output appears above this line, the errors have been fixed!" 