// Connects to a Yellowstone gRPC service to subscribe to filtered Solana blockchain transaction updates in real-time.

// Tracks token activity and market metrics (price changes, volume, buy/sell counts) using a TokenTracker.

// Manages token lists (whitelist/blacklist) with periodic review and persistence.

// Implements buying and selling logic through BuyManager and SellManager based on configurable thresholds like take profit and stop loss percentages.

// Cleans up inactive tokens and records token activity data periodically.

// Integrates with Telegram for sending notifications and receiving commands.

// Uses asynchronous Rust (tokio) for concurrency and efficient streaming data handling.