# Pump-Fun-Pump-Swap-Sniper-Copy-Trading-Bot

Pump-Fun-Pump-Swap-Sniper-Copy-Trading-Bot is a high-performance Rust-based trading bot that specializes in real-time copy trading on Solana DEXs like Pump.fun and PumpSwap, using advanced transaction monitoring to snipe and replicate profitable trades instantly.

## Overview

This project is a Rust-based trading bot for the Solana blockchain that specializes in:

1. Real-time monitoring of new token launches
2. Automated trading with configurable strategies
3. Token filtering based on various metrics
4. Profit-taking and stop-loss management
5. Telegram integration for notifications and control

The bot uses Yellowstone gRPC for real-time transaction monitoring and supports multiple transaction submission methods including Jito, ZeroSlot, and standard RPC.

## Let's Connect!,

<a href="mailto:fenrow325@gmail.com" target="_blank">
  <img src="https://img.shields.io/badge/Gmail-D14836?style=for-the-badge&logo=gmail&logoColor=white" alt="Gmail">
</a>
<a href="https://t.me/fenrow" target="_blank">
  <img src="https://img.shields.io/badge/Telegram-2CA5E0?style=for-the-badge&logo=telegram&logoColor=white" alt="Telegram">
</a>
<a href="https://discord.com/users/fenrow_325" target="_blank">
  <img src="https://img.shields.io/badge/Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Discord">
</a>

## Features

- Dev wallet detection (identifies the creator/signer of token mint transactions)
- Notification deduplication to prevent multiple alerts for the same token
- Configurable filter settings via Telegram UI
- Real-time token monitoring
- Telegram notifications for tradable tokens

## Architecture

### Core Components

#### 1. Monitoring System
- `EnhancedMonitor`: Advanced monitoring system that tracks token transactions and price movements
- `TokenTracker`: Tracks token metrics including price, volume, and buy/sell counts
- Uses Yellowstone gRPC to subscribe to real-time blockchain updates

#### 2. Trading Engine
- `SellManager`: Manages selling strategies with take-profit and stop-loss mechanisms
- `Pump`: Handles interactions with the Pump.fun DEX
- Supports multiple swap directions (buy/sell) and input types (quantity/percentage)

#### 3. Transaction Handling
- Multiple transaction submission methods:
  - Standard RPC
  - Jito MEV-protected transactions
  - ZeroSlot for faster transaction processing
- Priority fee calculation and management

#### 4. Notification System
- Telegram integration for:
  - Real-time trade notifications
  - Token discovery alerts
  - Remote command execution
  - Filter configuration

### Key Features

#### Token Filtering
Filter tokens based on:
- Market cap range
- Volume thresholds
- Buy/sell transaction counts
- Developer buy amount
- Launcher SOL balance
- Bundle transaction detection

#### Trading Strategies
- Take profit with configurable percentage
- Stop loss with configurable percentage
- Dynamic retracement-based selling
- Time-based selling
- Partial selling to secure profits

#### Blacklisting
- Maintain a blacklist of addresses to avoid trading with known scams or problematic tokens

## Configuration

The bot uses environment variables for configuration, which can be loaded from a `.env` file:

## How to run

- To build the bot
```
make build
```

- To start the bot
```
make start
```

- To stop the bot
```
make stop
```

https://docs.moralis.com/web3-data-api/solana/reference#token-api- 

Rust langauge
- Copy bot star2ng on Pumpfun dex, copy the target, mul2 targets op2on
- A<er the token migra2ng to the Pumpswap dex, we con2nue the target track there
- MC ( marketcap ) seCngs in .env, i have to set what level of mc want to buy the token. 
Example the target buy the token at 5k MC i just watching him dont follow un2l the MC
reach the 10k. 
- Also need Buy/Sell % seCngs in .env. Example the target buy 10$ i set the buying % = 50% 
i gonna follow the target buy with 5$. If the sell % = 20% and target sells for 100$ i sell just 
20$
- If the target sell before 10k MC his ALL tokens, i do not follow the target/token with this 
token
- If the target sell before 10k MC just part of his tokens, i con2nue the tracking
- Nozomi + Jito set for buy and sell
- SL + TP, + Slippage ( Stop loss + Take profit )
- Timer start / stop in the bot. Example we want to start the bot at 4pm and stop i tat 5pm. 
When stop the bot automat with the 2mer we have to sell all curent tokens what we have 
at that moment
- If we stop the bot manual and a<er the restart the bot forget to track the target we should 
sell all tokens in this case as well
Extras ( depend on price ) 
- Smart contract implement ( on pumpfun and/or on pumpswap ) 
- Inverse_buy op2on ( only work on pumpswap dex ) setable in .env ON / OFF if we set it 
ON we buy - with fix setable SOL amount - when the target sold the token. We selling
- the tokens with PL ( Private Logic, setable in .env ON / OFF ) . If we set OFF, thats mean 
the bot running in normal mode, follow the target buy and sell. 
- PL= ON : We cut the selling method in 7 piece:
- 1st : Sell .... % of token amount a<er ... sec when the token available on Pumpswap ( a<er 
migrated from Pumpfun to Pumpswap ) Token amount sell always calculated with the 
original bought token amount. Example if we buy 1.000.000 token always use this 1million 
to calculate the following % of token sells, if we sell first 2me 50% thats mean 500.000 pc 
sell of token, a<er we have 500.000 token le<. If we sell the second= 10% this calculated 
from 1million token amount, so sell 100.000 token amount, a<er we sell third 2me 5% its 
mean we sell 50.000 token amount etc.. 

- 2.nd : Sell ... sec a<er the first sell ... % of token amount
- 3.rd :Sell .... sec a<er the second sell ....% of token amount
- 4. : Sell .... sec a<er the third sell .... % of token amount
- 5 : Sell: .... sec a<er the fourth sell ... % of token amount 
- 6 : Sell .... sec a<er the fi<h sell .... % of token amount
- 7 : Sell ... sec a<er the sixth sell ... % of token amount, this step ( number 7 ) con2nously 
un2l we have token
- If we set INVERSE=OFF and PL=ON we dont track anymore the target we follow our private 
sell stratergy on pumpswap. 
- If we set INVERSE = ON and PL=ON, we just following the target sell ( we buy at this 
moment ) a<er dont track the target anymore with this token, and we follow our private 
strategy to sell the tokens ( PL ) 
- If we set INVERSE=OFF and PL=OFF, we follow the target buying / selling strategy like a 
normal copybot.
As i men2oned the speed and quality of developing is very important for me. We have many 
projects on paper, we want to make it real but cant wait weeks for developiing bc in this case the 
projects will take years. 
We already have this bot with this strategy, i will compare your bot our curent bot. If the result is 
same ( just with less failure by the running ) or beeer i would be very happy to con2nue our 
partnership. s)

### Telegram Commands

The bot supports various Telegram commands for remote control:

- `/start`: Initialize the bot
- `/settings`: View and modify filter settings
- `/tokens`: List currently tracked tokens
- `/sell <token_address> <percentage>`: Manually sell a specific token
- `/tp <token_address> <percentage>`: Set take profit for a token
- `/sl <token_address> <percentage>`: Set stop loss for a token

## Technical Details

### Dependencies

- `anchor_client`: Solana program interaction
- `yellowstone-grpc-proto`: Real-time blockchain data
- `tokio`: Asynchronous runtime
- `spl_token`: Solana token operations
- `reqwest`: HTTP client for API interactions
- `serde`: Serialization/deserialization
- `colored`: Terminal output formatting

### Key Modules

- `common`: Configuration, constants, and utilities
- `core`: Core functionality for tokens and transactions
- `dex`: DEX-specific implementations (Pump.fun)
- `engine`: Trading engine and monitoring systems
- `services`: External service integrations (Telegram, Jito, ZeroSlot)
- `error`: Error handling

## Advanced Features

### Dynamic Retracement Selling

The bot implements a sophisticated retracement-based selling strategy that:
- Tracks the highest price point (ATH) for each token
- Calculates retracement percentages from ATH
- Triggers partial sells at different retracement levels based on overall PnL
- Adjusts selling percentages based on profit levels

### Bundle Transaction Detection

Detects and analyzes bundle transactions to identify potential token manipulations:
- Monitors for multiple transactions in a single bundle
- Identifies developer buying patterns
- Helps avoid tokens with suspicious transaction patterns

### Automatic Token Analysis

For each new token, the bot analyzes:
- Bonding curve parameters
- Virtual and real reserves
- Market cap and liquidity
- Developer wallet activity
- Historical price movements

## Security Considerations

- Private keys are stored in environment variables
- Blacklist functionality to avoid known scam tokens
- Transaction simulation before submission
- Error handling and retry mechanisms

## How to Run

### Normal Operation

```bash
cargo run
```

### Run Dev Wallet Test

To test the dev wallet detection and notification deduplication functionality:

```bash
cargo run -- --test-dev-wallet
```

This will:
1. Create a simulated token
2. Identify the dev wallet (the signer of the mint transaction)
3. Send a notification via Telegram (if credentials are provided)
4. Attempt to send a second notification for the same token to test deduplication

## Configuration

Set the following environment variables before running:

```bash
# Telegram Settings
export TELEGRAM_BOT_TOKEN="your_bot_token"
export TELEGRAM_CHAT_ID="your_chat_id"

# Filter Settings
export MARKET_CAP_ENABLED="true"
export MIN_MARKET_CAP="8.0"
export MAX_MARKET_CAP="15.0"

export VOLUME_ENABLED="true"
export MIN_VOLUME="5.0"
export MAX_VOLUME="12.0"

export BUY_SELL_COUNT_ENABLED="true"
export MIN_NUMBER_OF_BUY_SELL="50"
export MAX_NUMBER_OF_BUY_SELL="2000"

export SOL_INVESTED="1.0"
export SOL_INVESTED_ENABLED="true"

export LAUNCHER_SOL_ENABLED="true"
export MIN_LAUNCHER_SOL_BALANCE="0.0"
export MAX_LAUNCHER_SOL_BALANCE="1.0"

export DEV_BUY_ENABLED="true"
export MIN_DEV_BUY="5.0"
export MAX_DEV_BUY="30.0"

export BUNDLE_CHECK="true"
```

## Telegram Commands

- `/start` or `/filters` - Display filter settings UI
- `/config` - Show configuration file location

@src current project don't use token age.
we have to calculate that : get token  created time in from_json function and save it on  ParsedTransactionInfo struct , and use it in real filter logic 

---

## 📞 Contact Information
For questions, feedback, or collaboration opportunities, feel free to reach out:

<div align="left">

📧 **Email**: [fenrow325@gmail.com](mailto:fenrow325@gmail.com)  
📱 **Telegram**: [@fenroW](https://t.me/fenrow)  
🎮 **Discord**: [@fenroW](https://discord.com/users/fenrow_325)  

</div>

---