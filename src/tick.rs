use soroban_sdk::{Env, contracttype};
use crate::DataKey;
use crate::math::snap_tick_to_spacing;

// ============================================================
// TICK DATA STRUCTURE (Uniswap V3 Style)
// ============================================================

#[contracttype]
#[derive(Clone, Debug)]
pub struct TickInfo {
    /// Total liquidity referencing this tick (for garbage collection)
    pub liquidity_gross: i128,
    /// Net liquidity change when crossing this tick left-to-right
    pub liquidity_net: i128,
    /// Fee growth per unit of liquidity on the OTHER side of this tick (token0)
    /// Flipped when the tick is crossed
    pub fee_growth_outside_0: u128,
    /// Fee growth per unit of liquidity on the OTHER side of this tick (token1)
    /// Flipped when the tick is crossed
    pub fee_growth_outside_1: u128,
    /// True if tick has been initialized
    pub initialized: bool,
}

impl Default for TickInfo {
    fn default() -> Self {
        TickInfo {
            liquidity_gross: 0,
            liquidity_net: 0,
            fee_growth_outside_0: 0,
            fee_growth_outside_1: 0,
            initialized: false,
        }
    }
}

// ============================================================
// STORAGE HELPERS
// ============================================================

pub fn read_tick_info(env: &Env, tick: i32) -> TickInfo {
    env.storage()
        .persistent()
        .get::<_, TickInfo>(&DataKey::Tick(tick))
        .unwrap_or_default()
}

pub fn write_tick_info(env: &Env, tick: i32, info: &TickInfo) {
    // Clear tick if no longer referenced by any position
    if info.liquidity_gross == 0 && !info.initialized {
        env.storage().persistent().remove(&DataKey::Tick(tick));
    } else {
        env.storage()
            .persistent()
            .set::<_, TickInfo>(&DataKey::Tick(tick), info);
    }
}

// ============================================================
// TICK UPDATE (Called when modifying liquidity)
// ============================================================

/// Update a tick and return true if the tick was flipped from uninitialized to initialized (or vice versa)
/// This follows Uniswap V3's Tick.update() pattern
pub fn update_tick(
    env: &Env,
    tick: i32,
    current_tick: i32,
    liquidity_delta: i128,
    fee_growth_global_0: u128,
    fee_growth_global_1: u128,
    upper: bool, // true if this is an upper tick boundary
) -> bool {
    let mut info = read_tick_info(env, tick);
    
    let liquidity_gross_before = info.liquidity_gross;
    let liquidity_gross_after = if liquidity_delta > 0 {
        liquidity_gross_before.saturating_add(liquidity_delta)
    } else {
        liquidity_gross_before.saturating_sub(liquidity_delta.abs())
    };
    
    // Tick flipped?
    let flipped = (liquidity_gross_after == 0) != (liquidity_gross_before == 0);
    
    // Initialize tick if crossing from 0 liquidity
    if liquidity_gross_before == 0 && liquidity_gross_after > 0 {
        // Initialize fee_growth_outside based on current tick position
        // By convention, if current tick >= this tick, we assume all fees were earned BELOW this tick
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

/// Find next initialized tick for swap iteration
/// Returns the next initialized tick in the given direction
pub fn find_next_initialized_tick(
    env: &Env,
    current_tick: i32,
    tick_spacing: i32,
    zero_for_one: bool,
) -> i32 {
    if tick_spacing <= 0 {
        return current_tick;
    }

    let step = if zero_for_one {
        -tick_spacing
    } else {
        tick_spacing
    };

    // Start from aligned tick
    let mut tick = snap_tick_to_spacing(current_tick, tick_spacing);
    
    // Move to next tick boundary
    tick = tick.saturating_add(step);

    let max_steps: i32 = 2000;
    for _ in 0..max_steps {
        // Check bounds
        if !(crate::math::MIN_TICK..=crate::math::MAX_TICK).contains(&tick) {
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
// TICK CROSSING (Uniswap V3 Style - Simplified)
// ============================================================

/// Cross a tick boundary during a swap
/// This is the core function that handles tick crossing
/// 
/// Key insight from Uniswap V3:
/// - fee_growth_outside represents fees accumulated on the "other side" of the tick
/// - When crossing, we flip it: new_outside = global - old_outside
/// - This automatically updates which side is "inside" vs "outside" for all positions
/// 
/// Returns the liquidity_net to add/subtract from active liquidity
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
// FEE GROWTH INSIDE CALCULATION (Uniswap V3 Style)
// ============================================================

/// Calculate fee growth inside a tick range
/// 
/// Formula: fee_growth_inside = global - below - above
/// 
/// Where:
/// - "below" is fees accumulated below lower_tick
/// - "above" is fees accumulated above upper_tick
/// 
/// The magic: fee_growth_outside at each tick is defined relative to current_tick
/// - If current_tick >= tick: outside = fees below tick
/// - If current_tick < tick: outside = fees above tick
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
// LEGACY HELPER (for backwards compatibility)
// ============================================================

/// Initialize tick if needed - simplified version
/// Prefer using update_tick() for new code
pub fn initialize_tick_if_needed(
    env: &Env,
    tick: i32,
    current_tick: i32,
    fee_growth_global_0: u128,
    fee_growth_global_1: u128,
) -> TickInfo {
    let mut info = read_tick_info(env, tick);

    // Only initialize if not yet done
    if !info.initialized && info.liquidity_gross == 0 {
        if current_tick >= tick {
            info.fee_growth_outside_0 = fee_growth_global_0;
            info.fee_growth_outside_1 = fee_growth_global_1;
        } else {
            info.fee_growth_outside_0 = 0;
            info.fee_growth_outside_1 = 0;
        }
        info.initialized = true;
    }

    info
}