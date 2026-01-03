# BelugaSwap

A Uniswap V3-style Automated Market Maker (AMM) with concentrated liquidity, built on Stellar/Soroban.

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Stellar](https://img.shields.io/badge/Stellar-Soroban-brightgreen)](https://stellar.org)

## Overview

BelugaSwap brings concentrated liquidity to the Stellar ecosystem. Unlike traditional AMMs that spread liquidity across the entire price curve (0 to ∞), BelugaSwap allows liquidity providers to concentrate their capital within specific price ranges, resulting in significantly higher capital efficiency.

### Key Features

- **Concentrated Liquidity** - Up to 4000x more capital efficient than traditional AMMs
- **Custom Price Ranges** - LPs choose exactly where to deploy their liquidity
- **Tick-based Architecture** - Efficient price discovery following Uniswap V3 design
- **Automatic Fee Accumulation** - Fees accumulate per position and can be collected anytime
- **Multi-tick Swaps** - Seamless swaps across multiple price ranges
- **Configurable Fees** - Flexible swap fee and protocol fee settings

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         BelugaSwap                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │   lib.rs    │  │  swap.rs    │  │  pool.rs    │              │
│  │             │  │             │  │             │              │
│  │ - initialize│  │ - swap      │  │ - PoolState │              │
│  │ - add_liq   │  │ - quote     │  │ - PoolConfig│              │
│  │ - remove_liq│  │ - validate  │  │ - read/write│              │
│  │ - collect   │  │             │  │             │              │
│  │ - swap      │  │             │  │             │              │
│  └─────────────┘  └─────────────┘  └─────────────┘              │
│         │               │               │                        │
│         └───────────────┼───────────────┘                        │
│                         │                                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │  tick.rs    │  │ position.rs │  │  math.rs    │              │
│  │             │  │             │  │             │              │
│  │ - TickInfo  │  │ - Position  │  │ - sqrt_price│              │
│  │ - cross_tick│  │ - update    │  │ - liquidity │              │
│  │ - fee_growth│  │ - fees calc │  │ - amounts   │              │
│  │   inside    │  │             │  │ - Q64.64    │              │
│  └─────────────┘  └─────────────┘  └─────────────┘              │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                        Data Flow                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  User                                                            │
│    │                                                             │
│    ▼                                                             │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐                   │
│  │   Swap   │───▶│  Tick    │───▶│ Position │                   │
│  │  Engine  │    │ Crossing │    │  Update  │                   │
│  └──────────┘    └──────────┘    └──────────┘                   │
│       │               │               │                          │
│       ▼               ▼               ▼                          │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐                   │
│  │  Price   │    │ Liquidity│    │   Fee    │                   │
│  │  Update  │    │  Update  │    │ Accrual  │                   │
│  └──────────┘    └──────────┘    └──────────┘                   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Core Components

| Module | Description |
|--------|-------------|
| `lib.rs` | Main contract entry points and orchestration |
| `swap.rs` | Swap execution engine with cross-tick support |
| `pool.rs` | Pool state and configuration management |
| `tick.rs` | Tick data structures and crossing logic |
| `position.rs` | LP position management and fee tracking |
| `math.rs` | Fixed-point math operations (Q64.64 format) |
| `twap.rs` | Time-weighted average price oracle (optional) |

### How Concentrated Liquidity Works

```
Traditional AMM (xy=k):
Liquidity spread from price 0 to ∞

Price:  0 ──────────────────────────────────────────▶ ∞
        ████████████████████████████████████████████
        └─────────────── Liquidity ─────────────────┘


BelugaSwap (Concentrated):
Liquidity concentrated in chosen price ranges

Price:  0 ────────────[=====]──────[========]───────▶ ∞
                      Position A    Position B
                      (tight)       (wide)

Result: Same capital, much deeper liquidity where it matters
```

## Prerequisites

### System Requirements

- **Rust** >= 1.74.0
- **Soroban CLI** >= 23.0.0
- **wasm32-unknown-unknown** target

### Installation

1. **Install Rust**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Add WASM target**
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

3. **Install Soroban CLI**
   ```bash
   cargo install --locked stellar-cli --features opt
   ```

4. **Configure Network**
   ```bash
   stellar network add testnet \
     --rpc-url https://soroban-testnet.stellar.org:443 \
     --network-passphrase "Test SDF Network ; September 2015"
   ```

5. **Create Test Accounts**
   ```bash
   stellar keys generate alice --network testnet
   stellar keys generate bob --network testnet
   ```

## Quick Start

### Build

```bash
cargo build --target wasm32-unknown-unknown --release
```

### Deploy

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/belugaswap.wasm \
  --source alice \
  --network testnet
```

### Initialize Pool

```bash
stellar contract invoke --id <CONTRACT_ID> --source alice --network testnet -- \
  initialize \
  --admin alice \
  --token_a <TOKEN_A_ADDRESS> \
  --token_b <TOKEN_B_ADDRESS> \
  --fee_bps 30 \
  --protocol_fee_bps 10 \
  --sqrt_price_x64 18446744073709551616 \
  --current_tick 0 \
  --tick_spacing 60
```

### Add Liquidity

```bash
stellar contract invoke --id <CONTRACT_ID> --source alice --network testnet -- \
  add_liquidity \
  --owner alice \
  --token_a <TOKEN_A> \
  --token_b <TOKEN_B> \
  --amount_a_desired 10000000 \
  --amount_b_desired 10000000 \
  --amount_a_min 0 \
  --amount_b_min 0 \
  --lower_tick -60 \
  --upper_tick 60
```

### Swap

```bash
stellar contract invoke --id <CONTRACT_ID> --source bob --network testnet -- \
  swap \
  --caller bob \
  --token_in <TOKEN_IN> \
  --token_out <TOKEN_OUT> \
  --amount_in 1000000 \
  --min_amount_out 900000 \
  --sqrt_price_limit_x64 0
```

## API Reference

### Core Functions

| Function | Description |
|----------|-------------|
| `initialize` | Initialize a new liquidity pool |
| `add_liquidity` | Add liquidity to a price range |
| `remove_liquidity` | Remove liquidity from a position |
| `swap` | Execute a token swap |
| `preview_swap` | Simulate a swap (read-only) |
| `collect` | Collect accumulated fees |

### View Functions

| Function | Description |
|----------|-------------|
| `get_pool_state` | Get current pool state |
| `get_position` | Get position details |
| `get_tick_info` | Get tick data |
| `get_swap_direction` | Determine swap direction |

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| `AMT_LOW` | Amount Too Low | Input amount below minimum threshold |
| `NO_LIQ` | No Liquidity | No liquidity available for swap |
| `SLIP_HI` | Slippage High | Output less than minimum specified |
| `OUT_DUST` | Output Dust | Output amount too small |
| `SLIP_MAX` | Max Slippage | Exceeds maximum allowed slippage |

## Technical Specifications

### Price Representation

- **Format**: Q64.64 fixed-point
- **sqrt_price**: `sqrt(price) * 2^64`
- **1:1 price**: `sqrt_price_x64 = 18446744073709551616` (2^64)

### Tick System

- **Tick Range**: -887,272 to +887,272
- **Price Formula**: `price = 1.0001^tick`
- **Each Tick**: ~0.01% price change

### Fee Structure

- **fee_bps**: Total swap fee (e.g., 30 = 0.30%)
- **protocol_fee_bps**: Protocol's share of fees (e.g., 10 = 10% of fees)

## Testing

```bash
# Run unit tests
cargo test

# Run with verbose output
cargo test -- --nocapture
```

## Project Structure

```
belugaswap/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Contract entry points
│   ├── math.rs         # Mathematical operations
│   ├── pool.rs         # Pool state management
│   ├── position.rs     # Position management
│   ├── swap.rs         # Swap engine
│   ├── tick.rs         # Tick management
│   └── twap.rs         # TWAP oracle
└── README.md
```

## Security

This software is provided as-is. While we strive for correctness, smart contracts carry inherent risks. Please review the code and use at your own risk.

For security concerns, please open an issue or contact the maintainers directly.

## License

```
Copyright 2024 BelugaSwap Contributors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

---

**Built with ❤️ from BelugaSwap**
