use soroban_sdk::{Env, Symbol};

use crate::constants::{
    MIN_SWAP_AMOUNT, MIN_OUTPUT_AMOUNT, MAX_SLIPPAGE_BPS, MAX_SWAP_ITERATIONS,
};
use crate::error::ErrorSymbol;
use crate::events::emit_sync_tick;
use crate::math::{compute_swap_step_with_target, get_sqrt_ratio_at_tick, div_q64};
use crate::storage::read_tick_info;
use crate::tick::{find_next_initialized_tick, cross_tick};
use crate::types::PoolState;

// ============================================================
// PUBLIC SWAP FUNCTIONS
// ============================================================

/// Execute a swap (modifies state)
/// 
/// # Arguments
/// * `env` - Soroban environment
/// * `pool` - Mutable pool state
/// * `amount_specified` - Input amount
/// * `zero_for_one` - Direction (true = token0 -> token1)
/// * `sqrt_price_limit_x64` - Price limit (0 for no limit)
/// * `fee_bps` - Fee in basis points
/// * `protocol_fee_bps` - Protocol fee in basis points
/// 
/// # Returns
/// (amount_in, amount_out)
/// 
/// # Panics
/// If swap amount is too small or no liquidity available
pub fn engine_swap(
    env: &Env,
    pool: &mut PoolState,
    amount_specified: i128,
    zero_for_one: bool,
    sqrt_price_limit_x64: u128,
    fee_bps: i128,
    protocol_fee_bps: i128,
) -> (i128, i128) {
    if amount_specified < MIN_SWAP_AMOUNT {
        panic!("swap amount too small");
    }

    if amount_specified <= 0 {
        return (0, 0);
    }

    if pool.liquidity <= 0 {
        panic!("no liquidity available");
    }

    engine_swap_internal(
        env,
        pool,
        amount_specified,
        zero_for_one,
        sqrt_price_limit_x64,
        fee_bps,
        protocol_fee_bps,
        true,   // allow_panic
        false,  // dry_run = false, actually modify state
    )
}

/// Quote a swap without executing it
/// 
/// # Returns
/// (amount_in_used, amount_out, final_sqrt_price)
pub fn quote_swap(
    env: &Env,
    pool: &PoolState,
    amount_in: i128,
    zero_for_one: bool,
    sqrt_price_limit_x64: u128,
    fee_bps: i128,
) -> (i128, i128, u128) {
    if amount_in < MIN_SWAP_AMOUNT || amount_in <= 0 || pool.liquidity <= 0 {
        return (0, 0, pool.sqrt_price_x64);
    }

    // Clone pool for simulation
    let mut sim_pool = pool.clone();

    let (amount_in_used, amount_out) = engine_swap_safe(
        env,
        &mut sim_pool,
        amount_in,
        zero_for_one,
        sqrt_price_limit_x64,
        fee_bps,
        0, // No protocol fee for quotes
    );

    (amount_in_used, amount_out, sim_pool.sqrt_price_x64)
}

/// Validate and preview a swap
/// 
/// # Returns
/// Ok((amount_in_used, amount_out, fee_paid, final_price)) or Err(Symbol)
pub fn validate_and_preview_swap(
    env: &Env,
    pool: &PoolState,
    amount_in: i128,
    min_amount_out: i128,
    zero_for_one: bool,
    sqrt_price_limit_x64: u128,
    fee_bps: i128,
) -> Result<(i128, i128, i128, u128), Symbol> {
    // Validate input amount
    if amount_in < MIN_SWAP_AMOUNT {
        return Err(ErrorSymbol::amt_low());
    }

    // Validate liquidity
    if pool.liquidity <= 0 {
        return Err(ErrorSymbol::no_liq());
    }

    // Get quote
    let (amount_in_used, amount_out, final_price) = quote_swap(
        env,
        pool,
        amount_in,
        zero_for_one,
        sqrt_price_limit_x64,
        fee_bps,
    );

    // Check slippage
    if amount_out < min_amount_out {
        return Err(ErrorSymbol::slip_hi());
    }

    // Check minimum output
    if amount_out < MIN_OUTPUT_AMOUNT {
        return Err(ErrorSymbol::out_dust());
    }

    // Calculate fee paid
    let fee_paid = amount_in_used.saturating_sub(amount_out);

    // Calculate slippage in basis points
    let slippage_bps = if amount_in_used > 0 {
        (amount_in.saturating_sub(amount_out))
            .saturating_mul(10000)
            .saturating_div(amount_in)
    } else {
        0
    };

    // Check maximum slippage
    if slippage_bps > MAX_SLIPPAGE_BPS {
        return Err(ErrorSymbol::slip_max());
    }

    Ok((amount_in_used, amount_out, fee_paid, final_price))
}

// ============================================================
// INTERNAL SWAP FUNCTIONS
// ============================================================

/// Safe swap version (returns (0,0) on error instead of panicking)
fn engine_swap_safe(
    env: &Env,
    pool: &mut PoolState,
    amount_specified: i128,
    zero_for_one: bool,
    sqrt_price_limit_x64: u128,
    fee_bps: i128,
    protocol_fee_bps: i128,
) -> (i128, i128) {
    if amount_specified < MIN_SWAP_AMOUNT || amount_specified <= 0 {
        return (0, 0);
    }

    if pool.liquidity <= 0 {
        return (0, 0);
    }

    engine_swap_internal(
        env,
        pool,
        amount_specified,
        zero_for_one,
        sqrt_price_limit_x64,
        fee_bps,
        protocol_fee_bps,
        false, // allow_panic
        true,  // dry_run - DON'T modify tick state!
    )
}

/// Core swap logic following Uniswap V3 pattern
/// 
/// # Arguments
/// * `dry_run` - If true, tick storage is NOT modified (for quotes)
fn engine_swap_internal(
    env: &Env,
    pool: &mut PoolState,
    amount_specified: i128,
    zero_for_one: bool,
    sqrt_price_limit_x64: u128,
    fee_bps: i128,
    protocol_fee_bps: i128,
    allow_panic: bool,
    dry_run: bool,
) -> (i128, i128) {
    // Initialize swap state
    let mut amount_remaining = amount_specified;
    let mut amount_out_total: i128 = 0;
    let mut total_protocol_fee: i128 = 0;

    let mut sqrt_price = pool.sqrt_price_x64;
    let mut liquidity = pool.liquidity;
    let mut current_tick = pool.current_tick;

    // Set default price limits
    let sqrt_limit = if sqrt_price_limit_x64 == 0 {
        if zero_for_one {
            100 // Minimum price
        } else {
            u128::MAX - 1000 // Maximum price
        }
    } else {
        sqrt_price_limit_x64
    };

    // Main swap loop
    let mut iterations = 0;

    while iterations < MAX_SWAP_ITERATIONS {
        iterations += 1;

        // Exit conditions
        if amount_remaining <= 0 {
            break;
        }

        if liquidity <= 0 {
            break;
        }

        // Check price limit
        if zero_for_one && sqrt_price <= sqrt_limit {
            break;
        }
        if !zero_for_one && sqrt_price >= sqrt_limit {
            break;
        }

        // Find next initialized tick
        let next_tick = find_next_initialized_tick(
            env,
            current_tick,
            pool.tick_spacing,
            zero_for_one,
        );

        // Get sqrt price at next tick
        let mut sqrt_target = get_sqrt_ratio_at_tick(next_tick);

        // Clamp target to user's price limit
        if zero_for_one {
            if sqrt_target < sqrt_limit {
                sqrt_target = sqrt_limit;
            }
        } else if sqrt_target > sqrt_limit {
            sqrt_target = sqrt_limit;
        }

        // Calculate fee divisor
        let fee_divisor = 10000 - fee_bps;
        if fee_divisor <= 0 {
            if allow_panic {
                panic!("fee too high");
            } else {
                break;
            }
        }

        // Amount available after fee reservation
        let amount_available = amount_remaining
            .saturating_mul(fee_divisor)
            .saturating_div(10000);

        if amount_available < MIN_OUTPUT_AMOUNT {
            break;
        }

        // Compute swap step
        let (sqrt_next, amount_in, amount_out) = if sqrt_price == sqrt_target {
            (sqrt_price, 0, 0)
        } else {
            compute_swap_step_with_target(
                env,
                sqrt_price,
                liquidity,
                amount_available,
                zero_for_one,
                sqrt_target,
            )
        };

        // Check minimum amounts
        if amount_in < MIN_OUTPUT_AMOUNT || amount_out < MIN_OUTPUT_AMOUNT {
            break;
        }

        // Calculate step fee
        let step_fee = calculate_step_fee(amount_in, amount_remaining, amount_available, fee_bps, fee_divisor);

        // Validate fee
        if step_fee < 0 || step_fee > amount_in {
            if allow_panic {
                panic!("invalid fee calculation");
            } else {
                break;
            }
        }

        // Calculate protocol fee
        let protocol_fee = if protocol_fee_bps > 0 && step_fee > 0 {
            step_fee.saturating_mul(protocol_fee_bps).saturating_div(10000)
        } else {
            0
        };

        let lp_fee = step_fee.saturating_sub(protocol_fee);

        // Update amounts
        amount_remaining = amount_remaining
            .saturating_sub(amount_in)
            .saturating_sub(step_fee);
        amount_out_total = amount_out_total.saturating_add(amount_out);
        total_protocol_fee = total_protocol_fee.saturating_add(protocol_fee);

        // Update fee growth global (Uniswap V3 style)
        if liquidity > 0 && lp_fee > 0 {
            let fee_u = lp_fee as u128;
            let liq_u = liquidity as u128;
            let growth_delta = div_q64(fee_u, liq_u);

            if zero_for_one {
                pool.fee_growth_global_0 = pool.fee_growth_global_0.wrapping_add(growth_delta);
            } else {
                pool.fee_growth_global_1 = pool.fee_growth_global_1.wrapping_add(growth_delta);
            }
        }

        // Handle tick crossing
        let target_reached = sqrt_next == sqrt_target;
        let should_cross = if zero_for_one {
            sqrt_target <= sqrt_price
        } else {
            sqrt_target >= sqrt_price
        };
        let at_user_limit = sqrt_price_limit_x64 != 0 && sqrt_target == sqrt_limit;

        if target_reached && should_cross && !at_user_limit {
            // Update price first
            sqrt_price = sqrt_target;

            // Cross tick - but only modify storage if NOT dry_run
            let liquidity_net = if dry_run {
                // For dry run (quotes), just read the liquidity_net without modifying storage
                let tick_info = read_tick_info(env, next_tick);
                tick_info.liquidity_net
            } else {
                // Actually cross the tick and modify storage
                cross_tick(
                    env,
                    next_tick,
                    pool.fee_growth_global_0,
                    pool.fee_growth_global_1,
                )
            };

            // Update liquidity based on direction
            if zero_for_one {
                // Moving left (price decreasing)
                liquidity = liquidity.saturating_sub(liquidity_net);
            } else {
                // Moving right (price increasing)
                liquidity = liquidity.saturating_add(liquidity_net);
            }

            // Update current tick
            current_tick = if zero_for_one {
                next_tick.saturating_sub(1)
            } else {
                next_tick
            };
        } else if sqrt_next != sqrt_price {
            // Moved within tick range
            sqrt_price = sqrt_next;

            if amount_remaining <= 0 {
                break;
            }
        } else {
            // No movement, exit loop
            break;
        }
    }

    // Validate output
    if amount_out_total < MIN_OUTPUT_AMOUNT {
        if allow_panic {
            panic!("output amount too small");
        } else {
            return (0, 0);
        }
    }

    // Update pool state
    pool.sqrt_price_x64 = sqrt_price;
    pool.liquidity = liquidity;
    pool.current_tick = current_tick;

    // Accumulate protocol fees
    if total_protocol_fee > 0 {
        if zero_for_one {
            pool.protocol_fees_0 = pool.protocol_fees_0.saturating_add(total_protocol_fee as u128);
        } else {
            pool.protocol_fees_1 = pool.protocol_fees_1.saturating_add(total_protocol_fee as u128);
        }
    }

    // Emit sync event
    emit_sync_tick(env, pool.current_tick, pool.sqrt_price_x64);

    let amount_in_total = amount_specified.saturating_sub(amount_remaining);
    (amount_in_total, amount_out_total)
}

// ============================================================
// HELPER FUNCTIONS
// ============================================================

/// Calculate the fee for a swap step
#[inline]
fn calculate_step_fee(
    amount_in: i128,
    amount_remaining: i128,
    amount_available: i128,
    fee_bps: i128,
    fee_divisor: i128,
) -> i128 {
    if amount_in == amount_available {
        // Used all available amount
        amount_remaining.saturating_sub(amount_in)
    } else {
        // Calculate fee on amount_in
        let fee_num = amount_in.saturating_mul(fee_bps);
        let fee = fee_num.saturating_div(fee_divisor);
        if fee_num % fee_divisor != 0 {
            fee.saturating_add(1) // Round up
        } else {
            fee
        }
    }
}