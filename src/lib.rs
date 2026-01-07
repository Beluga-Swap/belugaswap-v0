#![no_std]

use soroban_sdk::{contract, contractimpl, token, Address, Env};

// ============================================================
// MODULE DECLARATIONS
// ============================================================

mod constants;
mod error;
mod events;
mod math;
mod position;
mod storage;
mod swap;
mod tick;
mod types;

// ============================================================
// IMPORTS
// ============================================================

use constants::{MAX_FEE_BPS, MAX_PROTOCOL_FEE_BPS};
use error::{ErrorMsg, ErrorSymbol};
use events::{emit_initialized, emit_pool_init, emit_add_liquidity, emit_remove_liquidity, emit_swap, emit_collect};
use math::{get_amounts_for_liquidity, get_liquidity_for_amounts, snap_tick_to_spacing, MIN_LIQUIDITY, get_sqrt_ratio_at_tick};
use position::{read_position, write_position, update_position, modify_position, calculate_pending_fees, has_liquidity};
use storage::{
    is_initialized, set_initialized,
    read_pool_config, write_pool_config,
    read_pool_state, write_pool_state, init_pool_state,
};
use swap::{engine_swap, validate_and_preview_swap};
use tick::{get_fee_growth_inside, update_tick, is_valid_tick};
use types::{PoolConfig, PoolState, PositionInfo, SwapResult, PreviewResult, TickInfo};

// Re-export for external use
pub use storage::read_tick_info;

// ============================================================
// CONTRACT DEFINITION
// ============================================================

#[contract]
pub struct BelugaSwap;

#[contractimpl]
impl BelugaSwap {
    // ========================================================
    // INITIALIZATION
    // ========================================================

    /// Initialize the pool with configuration
    pub fn initialize(
        env: Env,
        admin: Address,
        token_a: Address,
        token_b: Address,
        fee_bps: u32,
        protocol_fee_bps: u32,
        sqrt_price_x64: u128,
        current_tick: i32,
        tick_spacing: i32,
    ) {
        admin.require_auth();

        if is_initialized(&env) {
            panic!("{}", ErrorMsg::ALREADY_INITIALIZED);
        }

        if fee_bps == 0 || fee_bps > MAX_FEE_BPS {
            panic!("{}", ErrorMsg::INVALID_FEE);
        }

        if protocol_fee_bps > MAX_PROTOCOL_FEE_BPS {
            panic!("{}", ErrorMsg::INVALID_PROTOCOL_FEE);
        }

        if tick_spacing <= 0 {
            panic!("{}", ErrorMsg::INVALID_TICK_SPACING);
        }

        // Validate tick is in valid range
        if !is_valid_tick(current_tick) {
            panic!("invalid initial tick");
        }

        // Sort tokens for consistent ordering
        let (token0, token1) = if token_a < token_b {
            (token_a.clone(), token_b.clone())
        } else {
            (token_b.clone(), token_a.clone())
        };

        let config = PoolConfig {
            admin,
            token_a,
            token_b,
            fee_bps,
            protocol_fee_bps,
        };
        write_pool_config(&env, &config);

        init_pool_state(&env, sqrt_price_x64, current_tick, tick_spacing, token0, token1);
        set_initialized(&env);

        emit_pool_init(&env, sqrt_price_x64, current_tick, tick_spacing);
        emit_initialized(&env, fee_bps, tick_spacing);
    }

    // ========================================================
    // VIEW FUNCTIONS
    // ========================================================

    /// Get pool state
    pub fn get_pool_state(env: Env) -> PoolState {
        read_pool_state(&env)
    }

    /// Get tick info
    pub fn get_tick_info(env: Env, tick: i32) -> TickInfo {
        storage::read_tick_info(&env, tick)
    }

    /// Get position info with pending fees
    pub fn get_position(env: Env, owner: Address, lower: i32, upper: i32) -> PositionInfo {
        let pos = read_position(&env, &owner, lower, upper);
        let pool = read_pool_state(&env);

        if !has_liquidity(&pos) {
            return PositionInfo {
                liquidity: 0,
                amount0: 0,
                amount1: 0,
                fees_owed_0: pos.tokens_owed_0,
                fees_owed_1: pos.tokens_owed_1,
            };
        }

        let sqrt_lower = get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = get_sqrt_ratio_at_tick(upper);

        let (amount0, amount1) = get_amounts_for_liquidity(
            &env, pos.liquidity, sqrt_lower, sqrt_upper, pool.sqrt_price_x64,
        );

        let (inside_0, inside_1) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick,
            pool.fee_growth_global_0, pool.fee_growth_global_1,
        );

        let (pending_0, pending_1) = calculate_pending_fees(&pos, inside_0, inside_1);

        PositionInfo {
            liquidity: pos.liquidity,
            amount0,
            amount1,
            fees_owed_0: pos.tokens_owed_0.saturating_add(pending_0),
            fees_owed_1: pos.tokens_owed_1.saturating_add(pending_1),
        }
    }

    /// Get swap direction for a given input token
    pub fn get_swap_direction(env: Env, token_in: Address) -> bool {
        let pool = read_pool_state(&env);
        token_in == pool.token0
    }

    // ========================================================
    // SWAP FUNCTIONS
    // ========================================================

    /// Swap tokens with automatic direction detection
    pub fn swap(
        env: Env,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        sqrt_price_limit_x64: u128,
    ) -> SwapResult {
        let pool = read_pool_state(&env);

        if token_in != pool.token0 && token_in != pool.token1 {
            panic!("{}", ErrorMsg::INVALID_TOKEN);
        }
        if token_out != pool.token0 && token_out != pool.token1 {
            panic!("{}", ErrorMsg::INVALID_TOKEN);
        }
        if token_in == token_out {
            panic!("{}", ErrorMsg::SAME_TOKEN);
        }

        let zero_for_one = token_in == pool.token0;

        Self::swap_advanced(env, caller, amount_in, min_amount_out, zero_for_one, sqrt_price_limit_x64)
    }

    /// Preview swap with automatic direction detection
    pub fn preview_swap(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        sqrt_price_limit_x64: u128,
    ) -> PreviewResult {
        let pool = read_pool_state(&env);

        if token_in != pool.token0 && token_in != pool.token1 {
            return PreviewResult {
                amount_in_used: 0, amount_out_expected: 0, fee_paid: 0,
                price_impact_bps: 0, is_valid: false,
                error_message: Some(ErrorSymbol::bad_token()),
            };
        }
        if token_out != pool.token0 && token_out != pool.token1 {
            return PreviewResult {
                amount_in_used: 0, amount_out_expected: 0, fee_paid: 0,
                price_impact_bps: 0, is_valid: false,
                error_message: Some(ErrorSymbol::bad_token()),
            };
        }
        if token_in == token_out {
            return PreviewResult {
                amount_in_used: 0, amount_out_expected: 0, fee_paid: 0,
                price_impact_bps: 0, is_valid: false,
                error_message: Some(ErrorSymbol::same_token()),
            };
        }

        let zero_for_one = token_in == pool.token0;
        Self::preview_swap_advanced(env, amount_in, min_amount_out, zero_for_one, sqrt_price_limit_x64)
    }

    /// Swap with manual direction control
    pub fn swap_advanced(
        env: Env,
        caller: Address,
        amount_specified: i128,
        min_amount_out: i128,
        zero_for_one: bool,
        sqrt_price_limit_x64: u128,
    ) -> SwapResult {
        caller.require_auth();

        let config = read_pool_config(&env);
        let mut pool = read_pool_state(&env);

        let fee_bps = config.fee_bps as i128;
        let protocol_fee_bps = config.protocol_fee_bps as i128;

        let validation = validate_and_preview_swap(
            &env, &pool, amount_specified, min_amount_out,
            zero_for_one, sqrt_price_limit_x64, fee_bps,
        );

        if let Err(e) = validation {
            panic!("{}: {:?}", ErrorMsg::SWAP_VALIDATION_FAILED, e);
        }

        let (amount_in_total, amount_out_total) = engine_swap(
            &env, &mut pool, amount_specified, zero_for_one,
            sqrt_price_limit_x64, fee_bps, protocol_fee_bps,
        );

        write_pool_state(&env, &pool);

        let pool_addr = env.current_contract_address();

        // Transfer tokens
        if zero_for_one {
            token::Client::new(&env, &pool.token0).transfer(&caller, &pool_addr, &amount_in_total);
            token::Client::new(&env, &pool.token1).transfer(&pool_addr, &caller, &amount_out_total);
        } else {
            token::Client::new(&env, &pool.token1).transfer(&caller, &pool_addr, &amount_in_total);
            token::Client::new(&env, &pool.token0).transfer(&pool_addr, &caller, &amount_out_total);
        }

        emit_swap(&env, amount_in_total, amount_out_total, zero_for_one);

        SwapResult {
            amount_in: amount_in_total,
            amount_out: amount_out_total,
            current_tick: pool.current_tick,
            sqrt_price_x64: pool.sqrt_price_x64,
        }
    }

    /// Preview swap with manual direction control
    pub fn preview_swap_advanced(
        env: Env,
        amount_specified: i128,
        min_amount_out: i128,
        zero_for_one: bool,
        sqrt_price_limit_x64: u128,
    ) -> PreviewResult {
        let config = read_pool_config(&env);
        let pool = read_pool_state(&env);
        let fee_bps = config.fee_bps as i128;

        let validation = validate_and_preview_swap(
            &env, &pool, amount_specified, min_amount_out,
            zero_for_one, sqrt_price_limit_x64, fee_bps,
        );

        match validation {
            Ok((amount_in_used, amount_out, fee_paid, final_price)) => {
                // Calculate price impact
                let price_impact = if amount_in_used > 0 && amount_out > 0 {
                    let diff = if amount_in_used > amount_out {
                        amount_in_used - amount_out
                    } else {
                        amount_out - amount_in_used
                    };
                    (diff * 10000) / amount_in_used
                } else {
                    0
                };

                // Price impact from sqrt_price change
                let sqrt_price_before = pool.sqrt_price_x64;
                let price_impact_from_sqrt = if sqrt_price_before > 0 {
                    let ratio_bps = if final_price > sqrt_price_before {
                        ((final_price - sqrt_price_before) * 10000) / sqrt_price_before
                    } else {
                        ((sqrt_price_before - final_price) * 10000) / sqrt_price_before
                    };
                    (ratio_bps * 2) as i128
                } else {
                    0
                };

                let final_price_impact = price_impact.max(price_impact_from_sqrt);

                PreviewResult {
                    amount_in_used,
                    amount_out_expected: amount_out,
                    fee_paid,
                    price_impact_bps: final_price_impact,
                    is_valid: true,
                    error_message: None,
                }
            }
            Err(e) => PreviewResult {
                amount_in_used: 0,
                amount_out_expected: 0,
                fee_paid: 0,
                price_impact_bps: 0,
                is_valid: false,
                error_message: Some(e),
            },
        }
    }

    // ========================================================
    // LIQUIDITY FUNCTIONS
    // ========================================================

    /// Add liquidity with automatic token ordering
    pub fn add_liquidity(
        env: Env,
        owner: Address,
        token_a: Address,
        token_b: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        lower_tick: i32,
        upper_tick: i32,
    ) -> (i128, i128, i128) {
        let pool = read_pool_state(&env);

        if (token_a != pool.token0 && token_a != pool.token1) || 
           (token_b != pool.token0 && token_b != pool.token1) {
            panic!("{}", ErrorMsg::INVALID_TOKEN);
        }
        if token_a == token_b {
            panic!("{}", ErrorMsg::SAME_TOKEN);
        }

        let (amount0_desired, amount1_desired, amount0_min, amount1_min) = 
            if token_a == pool.token0 {
                (amount_a_desired, amount_b_desired, amount_a_min, amount_b_min)
            } else {
                (amount_b_desired, amount_a_desired, amount_b_min, amount_a_min)
            };

        Self::add_liquidity_advanced(
            env, owner, lower_tick, upper_tick,
            amount0_desired, amount1_desired, amount0_min, amount1_min,
        )
    }

    /// Add liquidity with manual token0/token1 amounts
    pub fn add_liquidity_advanced(
        env: Env,
        owner: Address,
        lower_tick: i32,
        upper_tick: i32,
        amount0_desired: i128,
        amount1_desired: i128,
        amount0_min: i128,
        amount1_min: i128,
    ) -> (i128, i128, i128) {
        owner.require_auth();

        let mut pool = read_pool_state(&env);
        let pool_addr = env.current_contract_address();

        let lower = snap_tick_to_spacing(lower_tick, pool.tick_spacing);
        let upper = snap_tick_to_spacing(upper_tick, pool.tick_spacing);

        if lower >= upper {
            panic!("{}", ErrorMsg::INVALID_TICK_RANGE);
        }

        let sqrt_lower = get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = get_sqrt_ratio_at_tick(upper);

        let liquidity = get_liquidity_for_amounts(
            &env, amount0_desired, amount1_desired,
            sqrt_lower, sqrt_upper, pool.sqrt_price_x64,
        );

        if liquidity < MIN_LIQUIDITY {
            panic!("{}", ErrorMsg::LIQUIDITY_TOO_LOW);
        }

        let (amount0_actual, amount1_actual) = get_amounts_for_liquidity(
            &env, liquidity, sqrt_lower, sqrt_upper, pool.sqrt_price_x64,
        );

        if amount0_actual < amount0_min || amount1_actual < amount1_min {
            panic!("{}", ErrorMsg::SLIPPAGE_EXCEEDED);
        }

        // Update ticks FIRST to initialize fee_growth_outside properly
        update_tick(&env, lower, pool.current_tick, liquidity,
            pool.fee_growth_global_0, pool.fee_growth_global_1, false);
        update_tick(&env, upper, pool.current_tick, liquidity,
            pool.fee_growth_global_0, pool.fee_growth_global_1, true);

        // Get fee growth inside AFTER ticks are initialized
        let (inside_0, inside_1) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick,
            pool.fee_growth_global_0, pool.fee_growth_global_1,
        );

        // Update position
        let mut pos = read_position(&env, &owner, lower, upper);
        modify_position(&mut pos, liquidity, inside_0, inside_1);
        write_position(&env, &owner, lower, upper, &pos);

        // Update pool liquidity if position is in range
        if pool.current_tick >= lower && pool.current_tick < upper {
            pool.liquidity = pool.liquidity.saturating_add(liquidity);
        }
        write_pool_state(&env, &pool);

        // Transfer tokens
        if amount0_actual > 0 {
            token::Client::new(&env, &pool.token0).transfer(&owner, &pool_addr, &amount0_actual);
        }
        if amount1_actual > 0 {
            token::Client::new(&env, &pool.token1).transfer(&owner, &pool_addr, &amount1_actual);
        }

        emit_add_liquidity(&env, liquidity, amount0_actual, amount1_actual);

        (liquidity, amount0_actual, amount1_actual)
    }

    /// Remove liquidity from a position
    pub fn remove_liquidity(
        env: Env,
        owner: Address,
        lower_tick: i32,
        upper_tick: i32,
        liquidity_delta: i128,
    ) -> (i128, i128) {
        owner.require_auth();

        let mut pool = read_pool_state(&env);
        let pool_addr = env.current_contract_address();

        let lower = snap_tick_to_spacing(lower_tick, pool.tick_spacing);
        let upper = snap_tick_to_spacing(upper_tick, pool.tick_spacing);

        if liquidity_delta <= 0 {
            panic!("{}", ErrorMsg::INVALID_LIQUIDITY_AMOUNT);
        }

        let (inside_0, inside_1) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick,
            pool.fee_growth_global_0, pool.fee_growth_global_1,
        );

        let mut pos = read_position(&env, &owner, lower, upper);

        if liquidity_delta > pos.liquidity {
            panic!("{}", ErrorMsg::INSUFFICIENT_LIQUIDITY);
        }

        modify_position(&mut pos, -liquidity_delta, inside_0, inside_1);
        write_position(&env, &owner, lower, upper, &pos);

        update_tick(&env, lower, pool.current_tick, -liquidity_delta,
            pool.fee_growth_global_0, pool.fee_growth_global_1, false);
        update_tick(&env, upper, pool.current_tick, -liquidity_delta,
            pool.fee_growth_global_0, pool.fee_growth_global_1, true);

        if pool.current_tick >= lower && pool.current_tick < upper {
            pool.liquidity = pool.liquidity.saturating_sub(liquidity_delta);
        }
        write_pool_state(&env, &pool);

        let sqrt_lower = get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = get_sqrt_ratio_at_tick(upper);

        let (amount0, amount1) = get_amounts_for_liquidity(
            &env, liquidity_delta, sqrt_lower, sqrt_upper, pool.sqrt_price_x64,
        );

        if amount0 > 0 {
            token::Client::new(&env, &pool.token0).transfer(&pool_addr, &owner, &amount0);
        }
        if amount1 > 0 {
            token::Client::new(&env, &pool.token1).transfer(&pool_addr, &owner, &amount1);
        }

        emit_remove_liquidity(&env, liquidity_delta, amount0, amount1);

        (amount0, amount1)
    }

    // ========================================================
    // FEE COLLECTION
    // ========================================================

    /// Collect accumulated fees from a position
    pub fn collect(
        env: Env,
        owner: Address,
        lower_tick: i32,
        upper_tick: i32,
    ) -> (u128, u128) {
        owner.require_auth();

        let pool = read_pool_state(&env);
        let pool_addr = env.current_contract_address();

        let lower = snap_tick_to_spacing(lower_tick, pool.tick_spacing);
        let upper = snap_tick_to_spacing(upper_tick, pool.tick_spacing);

        let mut pos = read_position(&env, &owner, lower, upper);

        let (inside_0, inside_1) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick,
            pool.fee_growth_global_0, pool.fee_growth_global_1,
        );

        update_position(&mut pos, inside_0, inside_1);

        let amount0 = pos.tokens_owed_0;
        let amount1 = pos.tokens_owed_1;

        // Cap fees to available balance
        let pool_balance_0 = token::Client::new(&env, &pool.token0).balance(&pool_addr) as u128;
        let pool_balance_1 = token::Client::new(&env, &pool.token1).balance(&pool_addr) as u128;

        let amount0_capped = amount0.min(pool_balance_0);
        let amount1_capped = amount1.min(pool_balance_1);

        pos.tokens_owed_0 = pos.tokens_owed_0.saturating_sub(amount0_capped);
        pos.tokens_owed_1 = pos.tokens_owed_1.saturating_sub(amount1_capped);

        write_position(&env, &owner, lower, upper, &pos);

        if amount0_capped > 0 {
            token::Client::new(&env, &pool.token0).transfer(&pool_addr, &owner, &(amount0_capped as i128));
        }
        if amount1_capped > 0 {
            token::Client::new(&env, &pool.token1).transfer(&pool_addr, &owner, &(amount1_capped as i128));
        }

        emit_collect(&env, amount0_capped, amount1_capped);

        (amount0_capped, amount1_capped)
    }
}