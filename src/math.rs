//! # Math Module
//! 
//! Mathematical operations for BelugaSwap AMM.
//! All calculations use Q64.64 fixed-point format.
//! 
//! ## Q64.64 Format
//! - Integers are multiplied by 2^64
//! - Example: 1.0 = 2^64 = 18446744073709551616
//! 
//! ## Safety
//! - Uses U256 for intermediate calculations to prevent overflow
//! - All functions are designed to saturate rather than overflow

use soroban_sdk::{Env, U256};

use crate::constants::{MIN_TICK, MAX_TICK, Q64, MIN_LIQUIDITY as CONST_MIN_LIQUIDITY};

// ============================================================
// INTERNAL CONSTANTS
// ============================================================

const ONE_X64: u128 = Q64;

#[allow(dead_code)]
const MIN_PRICE_DELTA: u128 = 1;

/// Minimum liquidity (exported for use in lib.rs)
pub const MIN_LIQUIDITY: i128 = CONST_MIN_LIQUIDITY;

// ============================================================
// TYPE CONVERSION HELPERS
// ============================================================

/// Convert i128 to u128, returning 0 for negative values
#[inline]
fn i128_to_u128_safe(x: i128) -> u128 {
    if x <= 0 { 0 } else { x as u128 }
}

/// Convert u128 to i128 with saturation
#[inline]
fn u128_to_i128_saturating(x: u128) -> i128 {
    if x > i128::MAX as u128 { i128::MAX } else { x as i128 }
}

// ============================================================
// Q64.64 ARITHMETIC
// ============================================================

/// Multiply two Q64.64 numbers, returning Q64.64 result
/// Uses decomposition to avoid overflow
#[inline]
pub fn mul_q64(a: u128, b: u128) -> u128 {
    let a_hi = a >> 64;
    let a_lo = a & 0xFFFFFFFFFFFFFFFF;
    let b_hi = b >> 64;
    let b_lo = b & 0xFFFFFFFFFFFFFFFF;
    
    let term_hh = a_hi * b_hi;
    let term_hl = a_hi * b_lo;
    let term_lh = a_lo * b_hi;
    let term_ll = a_lo * b_lo;
    
    (term_hh << 64) + term_hl + term_lh + (term_ll >> 64)
}

/// Divide in Q64.64 format: (a * 2^64) / b
/// Returns Q64.64 result
#[inline]
pub fn div_q64(a: u128, b: u128) -> u128 {
    if b == 0 { return u128::MAX; }
    
    // Try direct calculation if no overflow
    if a <= (u128::MAX >> 64) {
        return (a << 64) / b;
    }
    
    // Decompose for large values
    let q = a / b;
    let r = a % b;
    
    let q_part = q << 64;
    let r_part = if r <= (u128::MAX >> 64) {
        (r << 64) / b
    } else {
        ((r >> 32) << 32) / (b >> 32).max(1)
    };
    
    q_part.saturating_add(r_part)
}

/// Safe multiply-divide using U256 to prevent overflow
/// Calculates: (a * b) / denominator
pub fn mul_div(env: &Env, a: u128, b: u128, denominator: u128) -> u128 {
    if denominator == 0 { panic!("mul_div: divide by zero"); }

    // Use U256 for intermediate calculation
    let a_256 = U256::from_u128(env, a);
    let b_256 = U256::from_u128(env, b);
    let den_256 = U256::from_u128(env, denominator);

    let product = a_256.mul(&b_256);
    let result = product.div(&den_256);

    result.to_u128().unwrap_or(u128::MAX)
}

/// Divide with rounding up
#[inline]
fn div_round_up(numerator: u128, denominator: u128) -> u128 {
    if denominator == 0 { return 0; }
    let result = numerator / denominator;
    if numerator % denominator != 0 {
        result.saturating_add(1)
    } else {
        result
    }
}

// ============================================================
// TICK UTILITIES
// ============================================================

/// Snap a tick to the nearest lower multiple of spacing
pub fn snap_tick_to_spacing(tick: i32, spacing: i32) -> i32 {
    if spacing <= 0 {
        panic!("tick_spacing must be positive");
    }
    let rem = tick.rem_euclid(spacing);
    tick - rem
}

/// Convert tick to sqrt price in Q64.64 format
/// Formula: sqrt(1.0001^tick) * 2^64
pub fn get_sqrt_ratio_at_tick(tick: i32) -> u128 {
    if !(MIN_TICK..=MAX_TICK).contains(&tick) { 
        panic!("tick out of range"); 
    }
    
    if tick == 0 { return ONE_X64; }
    
    let abs_tick = tick.unsigned_abs();
    let mut ratio: u128 = ONE_X64;
    
    // Binary decomposition of sqrt(1.0001^tick)
    // Each constant is sqrt(1.0001^(2^n)) * 2^64
    if abs_tick & 0x1 != 0 { ratio = mul_q64(ratio, 18447666387855958016); }
    if abs_tick & 0x2 != 0 { ratio = mul_q64(ratio, 18448588748116922368); }
    if abs_tick & 0x4 != 0 { ratio = mul_q64(ratio, 18450433606991732736); }
    if abs_tick & 0x8 != 0 { ratio = mul_q64(ratio, 18454123878217469952); }
    if abs_tick & 0x10 != 0 { ratio = mul_q64(ratio, 18461506635090006016); }
    if abs_tick & 0x20 != 0 { ratio = mul_q64(ratio, 18476281010653908992); }
    if abs_tick & 0x40 != 0 { ratio = mul_q64(ratio, 18505849059060717568); }
    if abs_tick & 0x80 != 0 { ratio = mul_q64(ratio, 18565033932859791360); }
    if abs_tick & 0x100 != 0 { ratio = mul_q64(ratio, 18683636815981789184); }
    if abs_tick & 0x200 != 0 { ratio = mul_q64(ratio, 18922376066158198784); }
    if abs_tick & 0x400 != 0 { ratio = mul_q64(ratio, 19403906064415539200); }
    if abs_tick & 0x800 != 0 { ratio = mul_q64(ratio, 20388321338895749120); }
    if abs_tick & 0x1000 != 0 { ratio = mul_q64(ratio, 22486086334269071360); }
    if abs_tick & 0x2000 != 0 { ratio = mul_q64(ratio, 27241267204663885824); }
    if abs_tick & 0x4000 != 0 { ratio = mul_q64(ratio, 40198444615281172480); }
    if abs_tick & 0x8000 != 0 { ratio = mul_q64(ratio, 87150709742682460160); }
    if abs_tick & 0x10000 != 0 { ratio = mul_q64(ratio, 409916713094318874624); }
    
    // For negative ticks, invert the ratio
    if tick < 0 {
        if ratio == 0 { return u128::MAX; }
        let numerator = ONE_X64.saturating_mul(ONE_X64); 
        ratio = numerator / ratio;
    }
    
    ratio
}

/// Alias for get_sqrt_ratio_at_tick
#[allow(dead_code)]
pub fn tick_to_sqrt_price_x64(_env: &Env, tick: i32) -> u128 { 
    get_sqrt_ratio_at_tick(tick) 
}

// ============================================================
// SQRT PRICE MATH
// ============================================================

/// Calculate next sqrt price given input amount
/// Used during swap to determine price movement
pub fn get_next_sqrt_price_from_input(
    env: &Env,
    sqrt_price: u128,
    liquidity: u128,
    amount_in: u128,
    zero_for_one: bool,
) -> u128 {
    if amount_in == 0 || liquidity == 0 {
        return sqrt_price;
    }

    if zero_for_one {
        // Token0 in -> Price decreases
        let product = amount_in.saturating_mul(sqrt_price);
        let numerator = liquidity.saturating_mul(sqrt_price);
        let liq_shifted = liquidity << 64; 
        let denominator = liq_shifted.saturating_add(product);
        
        if denominator == 0 { return sqrt_price; }
        mul_div(env, numerator, ONE_X64, denominator)
    } else {
        // Token1 in -> Price increases
        let quotient = div_q64(amount_in, liquidity);
        sqrt_price.saturating_add(quotient)
    }
}

/// Calculate next sqrt price given output amount
#[allow(dead_code)]
pub fn get_next_sqrt_price_from_output(
    env: &Env,
    sqrt_price: u128,
    liquidity: u128,
    amount_out: u128,
    zero_for_one: bool,
) -> u128 {
    if amount_out == 0 || liquidity == 0 {
        return sqrt_price;
    }

    if zero_for_one {
        // Token1 out -> Price decreases
        let quotient = div_q64(amount_out, liquidity); 
        sqrt_price.saturating_sub(quotient)
    } else {
        // Token0 out -> Price increases
        let product = amount_out.saturating_mul(sqrt_price);
        let numerator = liquidity.saturating_mul(sqrt_price);
        let liq_shifted = liquidity << 64;
        let denominator = liq_shifted.saturating_sub(product);
        
        if denominator == 0 { return u128::MAX; } 
        mul_div(env, numerator, ONE_X64, denominator)
    }
}

// ============================================================
// AMOUNT DELTA CALCULATIONS
// ============================================================

/// Calculate token0 amount for a liquidity and price range
/// Formula: L * (sqrt_upper - sqrt_lower) / (sqrt_upper * sqrt_lower)
pub fn get_amount_0_delta(
    sqrt_price_a: u128,
    sqrt_price_b: u128,
    liquidity: u128,
    round_up: bool,
) -> u128 {
    let (sqrt_lower, sqrt_upper) = if sqrt_price_a < sqrt_price_b {
        (sqrt_price_a, sqrt_price_b)
    } else {
        (sqrt_price_b, sqrt_price_a)
    };

    let delta_price = sqrt_upper.saturating_sub(sqrt_lower);
    let product_prices = mul_q64(sqrt_upper, sqrt_lower);
    
    if product_prices == 0 { return 0; }
    
    let numerator = liquidity.saturating_mul(delta_price);
    
    if round_up {
        div_round_up(numerator, product_prices)
    } else {
        numerator / product_prices
    }
}

/// Calculate token1 amount for a liquidity and price range
/// Formula: L * (sqrt_upper - sqrt_lower)
pub fn get_amount_1_delta(
    sqrt_price_a: u128,
    sqrt_price_b: u128,
    liquidity: u128,
    round_up: bool,
) -> u128 {
    let (sqrt_lower, sqrt_upper) = if sqrt_price_a < sqrt_price_b {
        (sqrt_price_a, sqrt_price_b)
    } else {
        (sqrt_price_b, sqrt_price_a)
    };

    let delta = sqrt_upper.saturating_sub(sqrt_lower);
    let product = liquidity.saturating_mul(delta);
    
    if round_up {
        if product & 0xFFFFFFFFFFFFFFFF != 0 {
            (product >> 64) + 1
        } else {
            product >> 64
        }
    } else {
        product >> 64
    }
}

// ============================================================
// SWAP STEP COMPUTATION
// ============================================================

/// Compute a single swap step
/// Returns: (next_sqrt_price, amount_in, amount_out)
#[allow(dead_code)]
pub fn compute_swap_step(
    env: &Env,
    sqrt_price_current: u128,
    liquidity: i128,
    amount_remaining: i128,
    zero_for_one: bool,
) -> (u128, i128, i128) {
    if liquidity <= 0 || amount_remaining <= 0 {
        return (sqrt_price_current, 0, 0);
    }
    
    let liq_u = i128_to_u128_safe(liquidity);
    let amt_in_remaining = i128_to_u128_safe(amount_remaining);
    
    let next_sqrt_price = get_next_sqrt_price_from_input(
        env,
        sqrt_price_current,
        liq_u,
        amt_in_remaining,
        zero_for_one
    );
    
    let price_delta = next_sqrt_price.abs_diff(sqrt_price_current);
    
    if price_delta < MIN_PRICE_DELTA {
        return (sqrt_price_current, 0, 0);
    }

    let amount_0 = get_amount_0_delta(sqrt_price_current, next_sqrt_price, liq_u, true);
    let amount_1 = get_amount_1_delta(sqrt_price_current, next_sqrt_price, liq_u, true);
    
    let (amount_in, amount_out) = if zero_for_one {
        (amount_0, amount_1)
    } else {
        (amount_1, amount_0)
    };
    
    let final_amount_in = amount_in.min(amt_in_remaining);

    (
        next_sqrt_price,
        u128_to_i128_saturating(final_amount_in),
        u128_to_i128_saturating(amount_out)
    )
}

/// Compute swap step with a target price
/// Used when approaching a tick boundary
/// Returns: (next_sqrt_price, amount_in, amount_out)
pub fn compute_swap_step_with_target(
    env: &Env,
    sqrt_price_current: u128,
    liquidity: i128,
    amount_specified: i128,
    zero_for_one: bool,
    sqrt_price_target: u128,
) -> (u128, i128, i128) {
    let liq_u = i128_to_u128_safe(liquidity);
    let amount_rem_u = i128_to_u128_safe(amount_specified);

    let next_price_input = get_next_sqrt_price_from_input(
        env,
        sqrt_price_current,
        liq_u,
        amount_rem_u,
        zero_for_one
    );

    // Check if we reach the target price
    let target_reached = if zero_for_one {
        next_price_input <= sqrt_price_target
    } else {
        next_price_input >= sqrt_price_target
    };

    let sqrt_price_next = if target_reached {
        sqrt_price_target
    } else {
        next_price_input
    };

    // Calculate amounts based on final price
    let (amount_in, amount_out) = if zero_for_one {
        (
            get_amount_0_delta(sqrt_price_current, sqrt_price_next, liq_u, true),
            get_amount_1_delta(sqrt_price_current, sqrt_price_next, liq_u, false),
        )
    } else {
        (
            get_amount_1_delta(sqrt_price_current, sqrt_price_next, liq_u, true),
            get_amount_0_delta(sqrt_price_current, sqrt_price_next, liq_u, false),
        )
    };

    // Cap amount_in to remaining amount if target not reached
    let final_amount_in = if !target_reached && amount_in > amount_rem_u {
        amount_rem_u
    } else {
        amount_in
    };

    (
        sqrt_price_next,
        u128_to_i128_saturating(final_amount_in),
        u128_to_i128_saturating(amount_out)
    )
}

// ============================================================
// LIQUIDITY CALCULATIONS
// ============================================================

/// Calculate liquidity from token0 amount
pub fn get_liquidity_for_amount0(
    _env: &Env, 
    amount0: i128, 
    sqrt_price_lower: u128, 
    sqrt_price_upper: u128
) -> i128 {
    if amount0 <= 0 || sqrt_price_lower >= sqrt_price_upper { return 0; }
    
    let amt0_u = i128_to_u128_safe(amount0);
    let product = mul_q64(sqrt_price_upper, sqrt_price_lower);
    let numerator = amt0_u.saturating_mul(product); 
    let denominator = sqrt_price_upper.saturating_sub(sqrt_price_lower);
    
    if denominator == 0 { return 0; }
    u128_to_i128_saturating(numerator / denominator)
}

/// Calculate liquidity from token1 amount
pub fn get_liquidity_for_amount1(
    _env: &Env, 
    amount1: i128, 
    sqrt_price_lower: u128, 
    sqrt_price_upper: u128
) -> i128 {
    if amount1 <= 0 || sqrt_price_lower >= sqrt_price_upper { return 0; }
    
    let amt1_u = i128_to_u128_safe(amount1);
    let diff = sqrt_price_upper.saturating_sub(sqrt_price_lower);
    
    if diff == 0 { return 0; }
    let liq_u = div_q64(amt1_u, diff);
    u128_to_i128_saturating(liq_u)
}

/// Calculate liquidity from both token amounts
/// Returns the minimum liquidity that can be provided with given amounts
pub fn get_liquidity_for_amounts(
    env: &Env,
    amount0_desired: i128, 
    amount1_desired: i128,
    sqrt_price_lower: u128, 
    sqrt_price_upper: u128, 
    current_sqrt_price: u128,
) -> i128 {
    if sqrt_price_lower >= sqrt_price_upper { return 0; }
    
    if current_sqrt_price <= sqrt_price_lower {
        // Current price below range: only token0 needed
        get_liquidity_for_amount0(env, amount0_desired, sqrt_price_lower, sqrt_price_upper)
    } else if current_sqrt_price >= sqrt_price_upper {
        // Current price above range: only token1 needed
        get_liquidity_for_amount1(env, amount1_desired, sqrt_price_lower, sqrt_price_upper)
    } else {
        // Current price in range: both tokens needed
        let liq0 = get_liquidity_for_amount0(env, amount0_desired, current_sqrt_price, sqrt_price_upper);
        let liq1 = get_liquidity_for_amount1(env, amount1_desired, sqrt_price_lower, current_sqrt_price);
        liq0.min(liq1)
    }
}

/// Calculate token amounts from liquidity
/// Returns: (amount0, amount1)
pub fn get_amounts_for_liquidity(
    _env: &Env, 
    liquidity: i128, 
    sqrt_price_lower: u128, 
    sqrt_price_upper: u128, 
    current_sqrt_price: u128,
) -> (i128, i128) {
    if liquidity <= 0 { return (0, 0); }
    
    let liq_u = i128_to_u128_safe(liquidity);
    
    // Clamp current price to range
    let sp = current_sqrt_price
        .max(sqrt_price_lower)
        .min(sqrt_price_upper);
    
    let amount0_u = if sp < sqrt_price_upper { 
        get_amount_0_delta(sp, sqrt_price_upper, liq_u, false) 
    } else { 
        0 
    };
    
    let amount1_u = if sp > sqrt_price_lower { 
        get_amount_1_delta(sqrt_price_lower, sp, liq_u, false) 
    } else { 
        0 
    };
    
    (u128_to_i128_saturating(amount0_u), u128_to_i128_saturating(amount1_u))
}