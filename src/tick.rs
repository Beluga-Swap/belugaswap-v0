use soroban_sdk::Env;

use crate::constants::{MIN_TICK, MAX_TICK, MAX_TICK_SEARCH_STEPS};
use crate::storage::{read_tick_info, write_tick_info};
use crate::math::snap_tick_to_spacing;

// ============================================================
// TICK UPDATE (Called when modifying liquidity)
// ============================================================

/// Update a tick when liquidity is added or removed
/// 
/// This follows Uniswap V3's Tick.update() pattern:
/// - Initializes fee_growth_outside based on current tick position
/// - Updates liquidity_gross and liquidity_net
/// - Returns true if tick was flipped (initialized/uninitialized)
/// 
/// # Arguments
/// * `env` - Soroban environment
/// * `tick` - Tick index to update
/// * `current_tick` - Current pool tick
/// * `liquidity_delta` - Liquidity change (positive = add, negative = remove)
/// * `fee_growth_global_0` - Current global fee growth for token0
/// * `fee_growth_global_1` - Current global fee growth for token1
/// * `upper` - True if this is an upper tick boundary
/// 
/// # Returns
/// True if the tick was flipped from uninitialized to initialized (or vice versa)
pub fn update_tick(
    env: &Env,
    tick: i32,
    current_tick: i32,
    liquidity_delta: i128,
    fee_growth_global_0: u128,
    fee_growth_global_1: u128,
    upper: bool,
) -> bool {
    let mut info = read_tick_info(env, tick);
    
    let liquidity_gross_before = info.liquidity_gross;
    let liquidity_gross_after = if liquidity_delta > 0 {
        liquidity_gross_before.saturating_add(liquidity_delta)
    } else {
        liquidity_gross_before.saturating_sub(liquidity_delta.abs())
    };
    
    // Check if tick was flipped
    let flipped = (liquidity_gross_after == 0) != (liquidity_gross_before == 0);
    
    // Initialize tick if crossing from 0 liquidity
    if liquidity_gross_before == 0 && liquidity_gross_after > 0 {
        // Initialize fee_growth_outside based on current tick position
        // Convention: if current_tick >= tick, assume all fees were earned BELOW this tick
        if current_tick >= tick {
            info.fee_growth_outside_0 = fee_growth_global_0;
            info.fee_growth_outside_1 = fee_growth_global_1;
        } else {
            // All fees were earned ABOVE this tick
            info.fee_growth_outside_0 = 0;
            info.fee_growth_outside_1 = 0;
        }
        info.initialized = true;
    }
    
    info.liquidity_gross = liquidity_gross_after;
    
    // Update liquidity_net
    // For lower tick: add liquidity (entering range from left)
    // For upper tick: subtract liquidity (exiting range from left)
    if upper {
        info.liquidity_net = info.liquidity_net.saturating_sub(liquidity_delta);
    } else {
        info.liquidity_net = info.liquidity_net.saturating_add(liquidity_delta);
    }
    
    // Clear initialized flag if no more liquidity
    if liquidity_gross_after == 0 {
        info.initialized = false;
    }
    
    write_tick_info(env, tick, &info);
    
    flipped
}

// ============================================================
// TICK TRAVERSAL
// ============================================================

/// Find the next initialized tick in the given direction
/// 
/// # Arguments
/// * `env` - Soroban environment
/// * `current_tick` - Starting tick
/// * `tick_spacing` - Pool's tick spacing
/// * `zero_for_one` - Direction (true = search left, false = search right)
/// 
/// # Returns
/// The next initialized tick, or current_tick if none found
pub fn find_next_initialized_tick(
    env: &Env,
    current_tick: i32,
    tick_spacing: i32,
    zero_for_one: bool,
) -> i32 {
    if tick_spacing <= 0 {
        return current_tick;
    }

    let step = if zero_for_one { -tick_spacing } else { tick_spacing };

    // Start from aligned tick
    let mut tick = snap_tick_to_spacing(current_tick, tick_spacing);
    
    // Move to next tick boundary
    tick = tick.saturating_add(step);

    for _ in 0..MAX_TICK_SEARCH_STEPS {
        // Check bounds
        if !(MIN_TICK..=MAX_TICK).contains(&tick) {
            return current_tick;
        }

        let info = read_tick_info(env, tick);
        
        // Found initialized tick with liquidity
        if info.initialized && info.liquidity_gross > 0 {
            return tick;
        }
        
        tick = tick.saturating_add(step);
    }

    current_tick
}

// ============================================================
// TICK CROSSING (Uniswap V3 Style)
// ============================================================

/// Cross a tick boundary during a swap
/// 
/// This is the core Uniswap V3 fee tracking mechanism:
/// - fee_growth_outside represents fees accumulated on the "other side" of the tick
/// - When crossing, we flip it: new_outside = global - old_outside
/// - This automatically updates which side is "inside" vs "outside" for all positions
/// 
/// # Arguments
/// * `env` - Soroban environment
/// * `tick` - Tick being crossed
/// * `fee_growth_global_0` - Current global fee growth for token0
/// * `fee_growth_global_1` - Current global fee growth for token1
/// 
/// # Returns
/// The liquidity_net to add/subtract from active liquidity
pub fn cross_tick(
    env: &Env,
    tick: i32,
    fee_growth_global_0: u128,
    fee_growth_global_1: u128,
) -> i128 {
    let mut info = read_tick_info(env, tick);
    
    // Flip fee_growth_outside
    // After crossing: outside = global - previous_outside
    // This is what makes Uniswap V3 fee tracking work!
    info.fee_growth_outside_0 = fee_growth_global_0.wrapping_sub(info.fee_growth_outside_0);
    info.fee_growth_outside_1 = fee_growth_global_1.wrapping_sub(info.fee_growth_outside_1);
    
    write_tick_info(env, tick, &info);
    
    info.liquidity_net
}

// ============================================================
// FEE GROWTH INSIDE CALCULATION
// ============================================================

/// Calculate fee growth inside a tick range
/// 
/// This is the key formula for calculating fees earned by a position:
/// fee_growth_inside = global - below - above
/// 
/// Where:
/// - "below" is fees accumulated below lower_tick
/// - "above" is fees accumulated above upper_tick
/// 
/// The magic: fee_growth_outside at each tick is defined relative to current_tick
/// - If current_tick >= tick: outside = fees below tick
/// - If current_tick < tick: outside = fees above tick
/// 
/// # Arguments
/// * `env` - Soroban environment
/// * `lower_tick` - Position's lower tick
/// * `upper_tick` - Position's upper tick
/// * `current_tick` - Pool's current tick
/// * `fee_growth_global_0` - Global fee growth for token0
/// * `fee_growth_global_1` - Global fee growth for token1
/// 
/// # Returns
/// (fee_growth_inside_0, fee_growth_inside_1)
pub fn get_fee_growth_inside(
    env: &Env,
    lower_tick: i32,
    upper_tick: i32,
    current_tick: i32,
    fee_growth_global_0: u128,
    fee_growth_global_1: u128,
) -> (u128, u128) {
    let lower_info = read_tick_info(env, lower_tick);
    let upper_info = read_tick_info(env, upper_tick);

    // Calculate fee_growth_below for lower tick
    let (fee_growth_below_0, fee_growth_below_1) = if current_tick >= lower_tick {
        // Current tick is at or above lower tick
        // Outside represents fees BELOW the tick
        (lower_info.fee_growth_outside_0, lower_info.fee_growth_outside_1)
    } else {
        // Current tick is below lower tick
        // Outside represents fees ABOVE the tick
        // So fees BELOW = global - outside
        (
            fee_growth_global_0.wrapping_sub(lower_info.fee_growth_outside_0),
            fee_growth_global_1.wrapping_sub(lower_info.fee_growth_outside_1),
        )
    };

    // Calculate fee_growth_above for upper tick
    let (fee_growth_above_0, fee_growth_above_1) = if current_tick < upper_tick {
        // Current tick is below upper tick
        // Outside represents fees ABOVE the tick
        (upper_info.fee_growth_outside_0, upper_info.fee_growth_outside_1)
    } else {
        // Current tick is at or above upper tick
        // Outside represents fees BELOW the tick
        // So fees ABOVE = global - outside
        (
            fee_growth_global_0.wrapping_sub(upper_info.fee_growth_outside_0),
            fee_growth_global_1.wrapping_sub(upper_info.fee_growth_outside_1),
        )
    };

    // fee_growth_inside = global - below - above
    let fee_growth_inside_0 = fee_growth_global_0
        .wrapping_sub(fee_growth_below_0)
        .wrapping_sub(fee_growth_above_0);

    let fee_growth_inside_1 = fee_growth_global_1
        .wrapping_sub(fee_growth_below_1)
        .wrapping_sub(fee_growth_above_1);

    (fee_growth_inside_0, fee_growth_inside_1)
}

// ============================================================
// TICK VALIDATION
// ============================================================

/// Check if a tick is within valid range
#[inline]
pub fn is_valid_tick(tick: i32) -> bool {
    tick >= MIN_TICK && tick <= MAX_TICK
}

/// Check if a tick is properly aligned to spacing
#[inline]
#[allow(dead_code)]
pub fn is_aligned_tick(tick: i32, tick_spacing: i32) -> bool {
    if tick_spacing <= 0 { return false; }
    tick % tick_spacing == 0
}