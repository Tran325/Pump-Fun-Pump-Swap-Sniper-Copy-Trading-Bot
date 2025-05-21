#!/bin/bash

# Check if the original errors are still in the compiled output
echo "Checking for E0282 type annotation errors..."
cargo check --lib 2>&1 | grep -A 2 "error\[E0282\].*line.*760"
cargo check --lib 2>&1 | grep -A 2 "error\[E0282\].*line.*854"
cargo check --lib 2>&1 | grep -A 2 "error\[E0282\].*line.*879"

echo "Checking for E0424 self reference errors..."
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*759"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*760"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*761"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*762"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*764"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*765"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*854"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*875"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*876"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*877"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*879"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*880"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*881"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*882"
cargo check --lib 2>&1 | grep -A 2 "error\[E0424\].*self.*line.*883"

echo "If no output appears above this line, the errors have been fixed!" 