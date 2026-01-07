// ============================================================
// TICK CONSTANTS
// ============================================================

/// Minimum valid tick value (corresponds to minimum price)
/// Price at MIN_TICK ≈ 2.94e-39
pub const MIN_TICK: i32 = -887272;

/// Maximum valid tick value (corresponds to maximum price)
/// Price at MAX_TICK ≈ 3.40e+38
pub const MAX_TICK: i32 = 887272;

// ============================================================
// SQRT PRICE CONSTANTS (Q64.64 format)
// ============================================================

/// Minimum sqrt price (at MIN_TICK)
/// sqrt(1.0001^-887272) * 2^64
#[allow(dead_code)]
pub const MIN_SQRT_PRICE: u128 = 4295128739;

/// Maximum sqrt price (at MAX_TICK) - using u128::MAX as safe upper bound
#[allow(dead_code)]
pub const MAX_SQRT_PRICE: u128 = u128::MAX;

/// sqrt price for 1:1 price ratio (2^64)
#[allow(dead_code)]
pub const SQRT_PRICE_1_1: u128 = 18446744073709551616_u128;

// ============================================================
// LIQUIDITY CONSTANTS
// ============================================================

/// Minimum liquidity for a position
pub const MIN_LIQUIDITY: i128 = 1000;

/// Maximum liquidity per tick to prevent overflow
#[allow(dead_code)]
pub const MAX_LIQUIDITY_PER_TICK: i128 = i128::MAX / 2;

// ============================================================
// SWAP CONSTANTS
// ============================================================

/// Minimum amount for a swap
pub const MIN_SWAP_AMOUNT: i128 = 1;

/// Minimum output amount (dust threshold)
pub const MIN_OUTPUT_AMOUNT: i128 = 1;

/// Maximum slippage in basis points (50% = 5000 bps)
pub const MAX_SLIPPAGE_BPS: i128 = 5000;

/// Maximum iterations in swap loop (prevents infinite loops)
pub const MAX_SWAP_ITERATIONS: u32 = 1024;

/// Maximum steps when searching for next initialized tick
pub const MAX_TICK_SEARCH_STEPS: i32 = 2000;

// ============================================================
// FEE CONSTANTS
// ============================================================

/// Maximum fee in basis points (100%)
pub const MAX_FEE_BPS: u32 = 10000;

/// Maximum protocol fee in basis points (100% of LP fees)
pub const MAX_PROTOCOL_FEE_BPS: u32 = 10000;

/// Default fee (0.3%)
#[allow(dead_code)]
pub const DEFAULT_FEE_BPS: u32 = 30;

/// Default protocol fee (10% of LP fees)
#[allow(dead_code)]
pub const DEFAULT_PROTOCOL_FEE_BPS: u32 = 10;

// ============================================================
// MATH CONSTANTS
// ============================================================

/// Q64 multiplier (2^64) for fixed-point math
pub const Q64: u128 = 1u128 << 64;

/// Q128 multiplier (2^128) for high-precision intermediate calculations
#[allow(dead_code)]
pub const Q128: u128 = 1u128 << 64 << 64;

/// Maximum reasonable fee delta (for overflow detection)
#[allow(dead_code)]
pub const MAX_REASONABLE_FEE_DELTA: u128 = 1u128 << 96;

// ============================================================
// TWAP CONSTANTS (Reserved for future use)
// ============================================================

/// Maximum TWAP observations stored
#[allow(dead_code)]
pub const MAX_TWAP_OBSERVATIONS: u32 = 100;

/// Minimum time between TWAP observations (in seconds)
#[allow(dead_code)]
pub const MIN_TWAP_OBSERVATION_INTERVAL: u64 = 1;