#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol,
};

mod math;
mod pool;
mod position;
mod swap;
mod tick;
mod twap;

use math::{
    get_amounts_for_liquidity, get_liquidity_for_amounts,
    snap_tick_to_spacing, MIN_LIQUIDITY,
};
use pool::{init_pool, read_pool_config, read_pool_state, write_pool_config, write_pool_state, PoolConfig, PoolState};
use position::{read_position, write_position, update_position, modify_position, calculate_pending_fees, Position};
use swap::{engine_swap, validate_and_preview_swap};
use tick::{
    get_fee_growth_inside, update_tick, read_tick_info, write_tick_info,
};

// ============================================================
// DATA KEYS
// ============================================================

#[contracttype]
pub enum DataKey {
    PoolState,
    PoolConfig,
    Initialized,
    Tick(i32),
    Position(Address, i32, i32),
    // TWAP keys
    TWAPObservation(u32),
    TWAPNewestIndex,
    TWAPInitialized,
}

// ============================================================
// RETURN TYPES
// ============================================================

#[contracttype]
#[derive(Clone, Debug)]
pub struct PositionInfo {
    pub liquidity: i128,
    pub amount0: i128,
    pub amount1: i128,
    pub fees_owed_0: u128,
    pub fees_owed_1: u128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SwapResult {
    pub amount_in: i128,
    pub amount_out: i128,
    pub current_tick: i32,
    pub sqrt_price_x64: u128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PreviewResult {
    pub amount_in_used: i128,
    pub amount_out_expected: i128,
    pub fee_paid: i128,
    pub price_impact_bps: i128,
    pub is_valid: bool,
    pub error_message: Option<Symbol>,
}

// ============================================================
// CONTRACT
// ============================================================

#[contract]
pub struct BelugaSwap;

#[contractimpl]
impl BelugaSwap {
    // ========================================================
    // INITIALIZATION
    // ========================================================

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

        if env.storage().persistent().has(&DataKey::Initialized) {
            panic!("already initialized");
        }

        if fee_bps == 0 || fee_bps > 10000 {
            panic!("invalid fee");
        }

        if protocol_fee_bps > 10000 {
            panic!("invalid protocol fee");
        }

        if tick_spacing <= 0 {
            panic!("invalid tick spacing");
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

        init_pool(&env, sqrt_price_x64, current_tick, tick_spacing, token0, token1);

        env.storage().persistent().set(&DataKey::Initialized, &true);

        env.events().publish(
            (Symbol::new(&env, "initialized"),),
            (fee_bps, tick_spacing),
        );
    }

    // ========================================================
    // VIEW POSITION - WITH PENDING FEES
    // ========================================================

    pub fn get_position(
        env: Env,
        owner: Address,
        lower: i32,
        upper: i32,
    ) -> PositionInfo {
        let pos = read_position(&env, &owner, lower, upper);
        let pool = read_pool_state(&env);

        if pos.liquidity == 0 {
            return PositionInfo {
                liquidity: 0,
                amount0: 0,
                amount1: 0,
                fees_owed_0: pos.tokens_owed_0,
                fees_owed_1: pos.tokens_owed_1,
            };
        }

        let sqrt_lower = math::get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = math::get_sqrt_ratio_at_tick(upper);

        let (amount0, amount1) = get_amounts_for_liquidity(
            &env,
            pos.liquidity,
            sqrt_lower,
            sqrt_upper,
            pool.sqrt_price_x64,
        );

        // Calculate pending fees
        let (inside_0, inside_1) = get_fee_growth_inside(
            &env,
            lower,
            upper,
            pool.current_tick,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
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

    // ========================================================
    // POOL STATE
    // ========================================================

    pub fn get_pool_state(env: Env) -> PoolState {
        read_pool_state(&env)
    }

    pub fn get_tick_info(env: Env, tick: i32) -> tick::TickInfo {
        read_tick_info(&env, tick)
    }

    // ========================================================
    // SWAP HELPERS
    // ========================================================

    /// Get swap direction for a given input token
    pub fn get_swap_direction(
        env: Env,
        token_in: Address,
    ) -> bool {
        let pool = read_pool_state(&env);
        token_in == pool.token0
    }

    // ========================================================
    // SWAP (MAIN - USER FRIENDLY)
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

        // Validate tokens
        if token_in != pool.token0 && token_in != pool.token1 {
            panic!("invalid token_in");
        }
        if token_out != pool.token0 && token_out != pool.token1 {
            panic!("invalid token_out");
        }
        if token_in == token_out {
            panic!("same token");
        }

        // Auto-detect direction
        let zero_for_one = token_in == pool.token0;

        // Call advanced swap
        Self::swap_advanced(
            env,
            caller,
            amount_in,
            min_amount_out,
            zero_for_one,
            sqrt_price_limit_x64,
        )
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

        // Validate tokens
        if token_in != pool.token0 && token_in != pool.token1 {
            return PreviewResult {
                amount_in_used: 0,
                amount_out_expected: 0,
                fee_paid: 0,
                price_impact_bps: 0,
                is_valid: false,
                error_message: Some(Symbol::new(&env, "bad_token")),
            };
        }
        if token_out != pool.token0 && token_out != pool.token1 {
            return PreviewResult {
                amount_in_used: 0,
                amount_out_expected: 0,
                fee_paid: 0,
                price_impact_bps: 0,
                is_valid: false,
                error_message: Some(Symbol::new(&env, "bad_token")),
            };
        }
        if token_in == token_out {
            return PreviewResult {
                amount_in_used: 0,
                amount_out_expected: 0,
                fee_paid: 0,
                price_impact_bps: 0,
                is_valid: false,
                error_message: Some(Symbol::new(&env, "same_token")),
            };
        }

        // Auto-detect direction
        let zero_for_one = token_in == pool.token0;

        // Call advanced preview
        Self::preview_swap_advanced(env, amount_in, min_amount_out, zero_for_one, sqrt_price_limit_x64)
    }

    // ========================================================
    // SWAP ADVANCED (FOR POWER USERS)
    // ========================================================

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
            &env,
            &pool,
            amount_specified,
            min_amount_out,
            zero_for_one,
            sqrt_price_limit_x64,
            fee_bps,
        );

        if let Err(e) = validation {
            panic!("swap validation failed: {:?}", e);
        }

        let (amount_in_total, amount_out_total) = engine_swap(
            &env,
            &mut pool,
            amount_specified,
            zero_for_one,
            sqrt_price_limit_x64,
            fee_bps,
            protocol_fee_bps,
        );

        write_pool_state(&env, &pool);

        let pool_addr = env.current_contract_address();

        // Transfer tokens
        if zero_for_one {
            // Swap token0 -> token1
            token::Client::new(&env, &pool.token0)
                .transfer(&caller, &pool_addr, &amount_in_total);
            token::Client::new(&env, &pool.token1)
                .transfer(&pool_addr, &caller, &amount_out_total);
        } else {
            // Swap token1 -> token0
            token::Client::new(&env, &pool.token1)
                .transfer(&caller, &pool_addr, &amount_in_total);
            token::Client::new(&env, &pool.token0)
                .transfer(&pool_addr, &caller, &amount_out_total);
        }

        env.events().publish(
            (Symbol::new(&env, "swap"),),
            (amount_in_total, amount_out_total, zero_for_one),
        );

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
            &env,
            &pool,
            amount_specified,
            min_amount_out,
            zero_for_one,
            sqrt_price_limit_x64,
            fee_bps,
        );

        match validation {
            Ok((amount_in_used, amount_out, fee_paid, _final_price)) => {
                let price_impact = if amount_specified > 0 {
                    ((amount_specified - amount_out) * 10000) / amount_specified
                } else {
                    0
                };

                PreviewResult {
                    amount_in_used,
                    amount_out_expected: amount_out,
                    fee_paid,
                    price_impact_bps: price_impact,
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
    // ADD LIQUIDITY (MAIN - USER FRIENDLY)
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

        // Validate tokens
        if (token_a != pool.token0 && token_a != pool.token1) || 
           (token_b != pool.token0 && token_b != pool.token1) {
            panic!("invalid tokens");
        }
        if token_a == token_b {
            panic!("same token");
        }

        // Map user's token_a/token_b to pool's token0/token1
        let (amount0_desired, amount1_desired, amount0_min, amount1_min) = 
            if token_a == pool.token0 {
                (amount_a_desired, amount_b_desired, amount_a_min, amount_b_min)
            } else {
                (amount_b_desired, amount_a_desired, amount_b_min, amount_a_min)
            };

        // Call advanced add_liquidity
        Self::add_liquidity_advanced(
            env,
            owner,
            lower_tick,
            upper_tick,
            amount0_desired,
            amount1_desired,
            amount0_min,
            amount1_min,
        )
    }

    // ========================================================
    // ADD LIQUIDITY ADVANCED (UNISWAP V3 STYLE)
    // ========================================================

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

        // Snap ticks to spacing
        let lower = snap_tick_to_spacing(lower_tick, pool.tick_spacing);
        let upper = snap_tick_to_spacing(upper_tick, pool.tick_spacing);

        if lower >= upper {
            panic!("invalid tick range");
        }

        let sqrt_lower = math::get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = math::get_sqrt_ratio_at_tick(upper);

        // Calculate liquidity from amounts
        let liquidity = get_liquidity_for_amounts(
            &env,
            amount0_desired,
            amount1_desired,
            sqrt_lower,
            sqrt_upper,
            pool.sqrt_price_x64,
        );

        if liquidity < MIN_LIQUIDITY {
            panic!("liquidity too low");
        }

        // Calculate actual amounts needed
        let (amount0_actual, amount1_actual) = get_amounts_for_liquidity(
            &env,
            liquidity,
            sqrt_lower,
            sqrt_upper,
            pool.sqrt_price_x64,
        );

        if amount0_actual < amount0_min || amount1_actual < amount1_min {
            panic!("slippage exceeded");
        }

        // IMPORTANT: Update ticks FIRST to initialize fee_growth_outside properly
        // This must happen BEFORE calculating fee_growth_inside
        let _lower_flipped = update_tick(
            &env,
            lower,
            pool.current_tick,
            liquidity,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
            false, // lower tick
        );

        let _upper_flipped = update_tick(
            &env,
            upper,
            pool.current_tick,
            liquidity,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
            true, // upper tick
        );

        // NOW get fee growth inside AFTER ticks are initialized
        let (inside_0, inside_1) = get_fee_growth_inside(
            &env,
            lower,
            upper,
            pool.current_tick,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
        );

        // Update position using Uniswap V3 pattern
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
            token::Client::new(&env, &pool.token0)
                .transfer(&owner, &pool_addr, &amount0_actual);
        }

        if amount1_actual > 0 {
            token::Client::new(&env, &pool.token1)
                .transfer(&owner, &pool_addr, &amount1_actual);
        }

        env.events().publish(
            (Symbol::new(&env, "add_liq"),),
            (liquidity, amount0_actual, amount1_actual),
        );

        (liquidity, amount0_actual, amount1_actual)
    }

    // ========================================================
    // REMOVE LIQUIDITY
    // ========================================================

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
            panic!("invalid liquidity amount");
        }

        // Get fee growth inside BEFORE modifying anything
        let (inside_0, inside_1) = get_fee_growth_inside(
            &env,
            lower,
            upper,
            pool.current_tick,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
        );

        let mut pos = read_position(&env, &owner, lower, upper);

        if liquidity_delta > pos.liquidity {
            panic!("insufficient liquidity");
        }

        // Update position fees and reduce liquidity
        modify_position(&mut pos, -liquidity_delta, inside_0, inside_1);
        write_position(&env, &owner, lower, upper, &pos);

        // Update ticks (negative liquidity delta)
        update_tick(
            &env,
            lower,
            pool.current_tick,
            -liquidity_delta,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
            false, // lower tick
        );

        update_tick(
            &env,
            upper,
            pool.current_tick,
            -liquidity_delta,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
            true, // upper tick
        );

        // Update pool liquidity if position was in range
        if pool.current_tick >= lower && pool.current_tick < upper {
            pool.liquidity = pool.liquidity.saturating_sub(liquidity_delta);
        }

        write_pool_state(&env, &pool);

        // Calculate amounts to return
        let sqrt_lower = math::get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = math::get_sqrt_ratio_at_tick(upper);

        let (amount0, amount1) = get_amounts_for_liquidity(
            &env,
            liquidity_delta,
            sqrt_lower,
            sqrt_upper,
            pool.sqrt_price_x64,
        );

        // Transfer tokens
        if amount0 > 0 {
            token::Client::new(&env, &pool.token0)
                .transfer(&pool_addr, &owner, &amount0);
        }

        if amount1 > 0 {
            token::Client::new(&env, &pool.token1)
                .transfer(&pool_addr, &owner, &amount1);
        }

        env.events().publish(
            (Symbol::new(&env, "remove_liq"),),
            (liquidity_delta, amount0, amount1),
        );

        (amount0, amount1)
    }

    // ========================================================
    // COLLECT FEES
    // ========================================================

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

        // Calculate fee_growth_inside with current state
        let (inside_0, inside_1) = get_fee_growth_inside(
            &env,
            lower,
            upper,
            pool.current_tick,
            pool.fee_growth_global_0,
            pool.fee_growth_global_1,
        );

        // Update position fees (this accumulates any pending fees)
        // The update_position function already handles underflow protection
        update_position(&mut pos, inside_0, inside_1);

        // Collect all owed tokens
        let amount0 = pos.tokens_owed_0;
        let amount1 = pos.tokens_owed_1;

        // SAFETY CHECK: Fees should never exceed pool balance
        // Get pool token balances
        let pool_balance_0 = token::Client::new(&env, &pool.token0)
            .balance(&pool_addr) as u128;
        let pool_balance_1 = token::Client::new(&env, &pool.token1)
            .balance(&pool_addr) as u128;

        // Cap fees to available balance
        let amount0_capped = amount0.min(pool_balance_0);
        let amount1_capped = amount1.min(pool_balance_1);

        // Update position with actual collected amounts
        pos.tokens_owed_0 = pos.tokens_owed_0.saturating_sub(amount0_capped);
        pos.tokens_owed_1 = pos.tokens_owed_1.saturating_sub(amount1_capped);

        write_position(&env, &owner, lower, upper, &pos);

        // Transfer tokens (only if > 0)
        if amount0_capped > 0 {
            token::Client::new(&env, &pool.token0)
                .transfer(&pool_addr, &owner, &(amount0_capped as i128));
        }

        if amount1_capped > 0 {
            token::Client::new(&env, &pool.token1)
                .transfer(&pool_addr, &owner, &(amount1_capped as i128));
        }

        env.events().publish(
            (Symbol::new(&env, "collect"),),
            (amount0_capped, amount1_capped),
        );

        (amount0_capped, amount1_capped)
    }
}