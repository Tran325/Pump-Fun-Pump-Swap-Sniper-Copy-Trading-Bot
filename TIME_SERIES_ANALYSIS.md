# Time Series Analysis & Advanced Trading Strategies

## Overview

This implementation enhances the existing pump.fun token trading bot with comprehensive time series analysis capabilities, sophisticated entry and exit criteria, advanced risk management, and market-aware trading strategies.

## Key Components

### Time Series Analysis Module
- **Data Collection**: Efficiently captures and stores token price, volume, and transaction history.
- **Technical Indicators**: Calculates various technical indicators including:
  - Simple Moving Averages (SMA) - 5, 10, 20, 50 periods
  - Exponential Moving Averages (EMA) - 5, 10, 20, 50 periods
  - Relative Strength Index (RSI)
  - Moving Average Convergence Divergence (MACD)
  - Bollinger Bands
  - Price Momentum
  - Rate of Change
- **Signal Generation**: Produces actionable buy/sell signals with confidence scores.
- **Serialization Support**: Properly handles data persistence with serializable types.

### Enhanced Entry Criteria
- **Momentum-Based Entry**: Identifies tokens with positive price momentum.
- **Buy/Sell Ratio Analysis**: Favors tokens with strong buying pressure.
- **RSI Conditions**: Buys oversold tokens (RSI < 30) and sells overbought tokens (RSI > 70).
- **Moving Average Crossovers**: Detects golden crosses (short-term MA crossing above long-term MA) for entries.
- **Bollinger Band Rebounds**: Identifies price rebounds from lower Bollinger Band.
- **Token Age Verification**: Validates that tokens are indeed new launches.

### Dynamic Exit Strategies
- **Volatility-Adjusted Take Profit**: Sets take profit levels based on Bollinger Band width.
- **Multiple Profit Targets**: Implements partial exits at various profit levels.
- **Trailing Stops**: Dynamically adjusts stop loss as price increases.
- **Time-Based Exit Adjustments**: Tightens stops the longer a position is held.

### Advanced Risk Management
- **Position Sizing**: Calculates position size based on:
  - Technical indicator confidence levels
  - Portfolio risk percentage
  - Token volatility
- **Daily Budget Controls**: Enforces maximum daily exposure.
- **Risk-Adjusted Stop Losses**: Tighter stops for higher-risk tokens.

### Bonding Curve Analysis
- **Curve Steepness Evaluation**: Identifies optimal entry points on bonding curves.
- **Liquidity Depth Analysis**: Ensures sufficient liquidity before trade execution.
- **Price Impact Calculation**: Estimates slippage for different position sizes.

## Configuration Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| MIN_BUY_CONFIDENCE | 65 | Minimum confidence score to execute a buy (0-100) |
| MIN_SELL_CONFIDENCE | 70 | Minimum confidence score to execute a sell (0-100) |
| DAILY_BUY_BUDGET | 2.0 | Maximum SOL to spend per day |
| MAX_TIME_SERIES_POINTS | 100 | Number of data points to store per token |
| TIME_SERIES_INTERVAL_SECS | 15 | Interval between data points in seconds |
| TAKE_PROFIT_PERCENT | 20.0 | Default take profit percentage |
| STOP_LOSS_PERCENT | 10.0 | Default stop loss percentage |

## Integration

The time series analysis module seamlessly integrates with the existing token trading system:

1. **Data Collection**: Token data is continuously collected from the token tracker.
2. **Signal Generation**: Technical analysis generates buy/sell signals with confidence scores.
3. **Buy Execution**: High-confidence buy signals are routed to the buy manager.
4. **Dynamic Exit Parameters**: Exit parameters are adjusted based on market conditions.
5. **Sell Execution**: Sell signals trigger the sell manager with optimized parameters.

## Usage

The enhanced trading system can be started using:

```rust
start_enhanced_trading_system(
    app_state,
    swap_config,
    blacklist_enabled,
    take_profit_percent,
    stop_loss_percent,
    telegram_bot_token,
    telegram_chat_id,
).await
```

## Technical Indicator Implementation Details

### Simple Moving Average (SMA)
Calculates the average price over a specified period.

```rust
fn calculate_sma(&self, prices: &[f64], period: usize) -> f64 {
    if prices.len() < period {
        return 0.0;
    }
    
    let sum: f64 = prices[prices.len() - period..].iter().sum();
    sum / period as f64
}
```

### Exponential Moving Average (EMA)
Gives more weight to recent prices for faster response to price changes.

```rust
fn calculate_ema(&self, prices: &[f64], period: usize) -> f64 {
    if prices.len() < period {
        return 0.0;
    }
    
    let mut ema = self.calculate_sma(prices, period);
    let multiplier = 2.0 / (period as f64 + 1.0);
    
    for i in prices.len() - period + 1..prices.len() {
        ema = (prices[i] - ema) * multiplier + ema;
    }
    
    ema
}
```

### Relative Strength Index (RSI)
Measures the speed and change of price movements, indicating overbought/oversold conditions.

```rust
fn calculate_rsi(&self, prices: &[f64], period: usize) -> f64 {
    if prices.len() < period + 1 {
        return 50.0; // Default to neutral
    }
    
    let mut gains = 0.0;
    let mut losses = 0.0;
    
    for i in prices.len() - period..prices.len() {
        let change = prices[i] - prices[i - 1];
        if change >= 0.0 {
            gains += change;
        } else {
            losses -= change;
        }
    }
    
    if losses == 0.0 {
        return 100.0; // All gains, no losses
    }
    
    let rs = gains / losses;
    100.0 - (100.0 / (1.0 + rs))
}
```

### Bollinger Bands
Shows price volatility and potential reversal points using standard deviation.

```rust
fn calculate_bollinger_bands(&self, prices: &[f64], period: usize, std_dev_multiplier: f64) -> (f64, f64, f64) {
    if prices.len() < period {
        let price = prices.last().unwrap_or(&0.0);
        return (*price, *price, *price);
    }
    
    // Calculate SMA
    let sma = self.calculate_sma(prices, period);
    
    // Calculate Standard Deviation
    let price_slice = &prices[prices.len() - period..];
    let sum_squares: f64 = price_slice.iter().map(|p| (p - sma).powi(2)).sum();
    let std_dev = (sum_squares / period as f64).sqrt();
    
    // Calculate bands
    let upper_band = sma + (std_dev_multiplier * std_dev);
    let lower_band = sma - (std_dev_multiplier * std_dev);
    
    (sma, upper_band, lower_band)
}
```

## Performance and Optimization

The implementation is designed for efficiency with:

- **Interval-Based Data Collection**: Controls data frequency to avoid excessive CPU usage.
- **Fixed-Size Data Storage**: Limits memory usage by maintaining a fixed-size data window.
- **Mutex Guards**: Properly managed to avoid deadlocks across async boundaries.
- **Isolated Function Pattern**: Ensures MutexGuard isn't held across await points.
- **Serializable Data Structures**: Time series data can be serialized for persistence.

## Data Serialization

To handle persistence, we've implemented serializable versions of our data structures:

```rust
// For internal use with timestamps
#[derive(Debug, Clone)]
pub struct TimeSeriesDataPoint {
    pub timestamp: Instant,  // Non-serializable std::time::Instant
    pub price: f64,
    pub volume: f64,
    pub buy_count: u32,
    pub sell_count: u32,
    pub market_cap: Option<f64>,
    pub iso_timestamp: DateTime<Utc>,  // For serialization
}

// For persistence/serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableTimeSeriesDataPoint {
    pub timestamp: String,  // ISO 8601 timestamp string
    pub price: f64,
    pub volume: f64,
    pub buy_count: u32,
    pub sell_count: u32,
    pub market_cap: Option<f64>,
}
```

## Extending the System

The time series analysis system is designed for easy extension:

1. **Adding New Indicators**: Implement additional technical indicators by adding new calculation methods.
2. **Custom Signal Logic**: Develop sophisticated signal generation by modifying `generate_signals()`.
3. **Risk Profile Integration**: Connect with external risk assessment systems for better position sizing.
4. **Market-Wide Analysis**: Incorporate Solana ecosystem-wide metrics for market sentiment. 
5. **Data Persistence**: Implement storage and retrieval using the serializable data structures. 