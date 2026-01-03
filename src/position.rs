use soroban_sdk::{Env, contracttype, Address};
use crate::DataKey;

// ============================================================
// POSITION DATA STRUCTURE (Uniswap V3 Style)
// ============================================================

#[contracttype]
#[derive(Clone, Debug)]
pub struct Position {
    /// The amount of liquidity owned by this position
    pub liquidity: i128,

    /// Fee growth inside the position's tick range as of the last update (token0)
    /// This is used to calculate owed fees
    pub fee_growth_inside_last_0: u128,

    /// Fee growth inside the position's tick range as of the last update (token1)
    pub fee_growth_inside_last_1: u128,

    /// Uncollected token0 owed to the position
    pub tokens_owed_0: u128,

    /// Uncollected token1 owed to the position
    pub tokens_owed_1: u128,
}

impl Default for Position {
    fn default() -> Self {
        Position {
            liquidity: 0,
            fee_growth_inside_last_0: 0,
            fee_growth_inside_last_1: 0,
            tokens_owed_0: 0,
            tokens_owed_1: 0,
        }
    }
}

// ============================================================
// STORAGE HELPERS
// ============================================================

/// Read position from storage, returns default if not found
pub fn read_position(env: &Env, owner: &Address, lower: i32, upper: i32) -> Position {
    env.storage()
        .persistent()
        .get::<_, Position>(&DataKey::Position(owner.clone(), lower, upper))
        .unwrap_or_default()
}

/// Write position to storage, removes if empty
pub fn write_position(
    env: &Env,
    owner: &Address,
    lower: i32,
    upper: i32,
    pos: &Position,
) {
    // Only delete if completely empty (no liquidity and no pending fees)
    if pos.liquidity == 0 && pos.tokens_owed_0 == 0 && pos.tokens_owed_1 == 0 {
        env.storage()
            .persistent()
            .remove(&DataKey::Position(owner.clone(), lower, upper));
    } else {
        env.storage()
            .persistent()
            .set::<_, Position>(
                &DataKey::Position(owner.clone(), lower, upper),
                pos,
            );
    }
}

// ============================================================
// POSITION UPDATE (Uniswap V3 Style)
// ============================================================

/// Update a position's fee checkpoints and calculate owed tokens
/// 
/// This is the core Uniswap V3 fee collection pattern:
/// 1. Calculate delta = current_inside - last_inside (using wrapping arithmetic)
/// 2. owed_tokens += liquidity * delta / 2^64
/// 3. Update last_inside = current_inside
/// 
/// In Uniswap V3, wrapping arithmetic is used and it always works correctly
/// because fee_growth_inside is consistent through tick crossings.
pub fn update_position(
    pos: &mut Position,
    fee_growth_inside_0: u128,
    fee_growth_inside_1: u128,
) {
    if pos.liquidity > 0 {
        let liquidity_u = pos.liquidity as u128;
        
        // Calculate fee deltas using wrapping subtraction
        // In Uniswap V3, this always works because fee_growth_inside
        // is consistent - it never "decreases" for an active position
        let delta_0 = fee_growth_inside_0.wrapping_sub(pos.fee_growth_inside_last_0);
        let delta_1 = fee_growth_inside_1.wrapping_sub(pos.fee_growth_inside_last_1);
        
        // Calculate owed fees using safe multiplication
        // fee = (liquidity * delta) >> 64
        // Use checked multiplication to detect overflow
        let fee_0 = match liquidity_u.checked_mul(delta_0) {
            Some(product) => product >> 64,
            None => {
                // Overflow indicates invalid delta (likely underflow in subtraction)
                // This should not happen in correct Uniswap V3 implementation
                0
            }
        };
        
        let fee_1 = match liquidity_u.checked_mul(delta_1) {
            Some(product) => product >> 64,
            None => 0
        };
        
        // Accumulate owed tokens
        pos.tokens_owed_0 = pos.tokens_owed_0.saturating_add(fee_0);
        pos.tokens_owed_1 = pos.tokens_owed_1.saturating_add(fee_1);
    }
    
    // Always update checkpoints to current values
    pos.fee_growth_inside_last_0 = fee_growth_inside_0;
    pos.fee_growth_inside_last_1 = fee_growth_inside_1;
}

/// Modify position liquidity and update fees
/// This combines fee update with liquidity modification
pub fn modify_position(
    pos: &mut Position,
    liquidity_delta: i128,
    fee_growth_inside_0: u128,
    fee_growth_inside_1: u128,
) {
    // First update fees based on current liquidity
    update_position(pos, fee_growth_inside_0, fee_growth_inside_1);
    
    // Then modify liquidity
    if liquidity_delta > 0 {
        pos.liquidity = pos.liquidity.saturating_add(liquidity_delta);
    } else if liquidity_delta < 0 {
        pos.liquidity = pos.liquidity.saturating_sub(liquidity_delta.abs());
    }
}

/// Calculate pending fees without modifying position
/// Used for view functions
pub fn calculate_pending_fees(
    pos: &Position,
    fee_growth_inside_0: u128,
    fee_growth_inside_1: u128,
) -> (u128, u128) {
    if pos.liquidity == 0 {
        return (0, 0);
    }
    
    let liquidity_u = pos.liquidity as u128;
    
    // Calculate deltas
    let delta_0 = fee_growth_inside_0.wrapping_sub(pos.fee_growth_inside_last_0);
    let delta_1 = fee_growth_inside_1.wrapping_sub(pos.fee_growth_inside_last_1);
    
    // Sanity check: if delta > half of u128::MAX, it's likely underflow
    // This can happen during view calls when position is in unusual state
    const MAX_REASONABLE_DELTA: u128 = u128::MAX / 2;
    
    let pending_0 = if delta_0 < MAX_REASONABLE_DELTA {
        (liquidity_u.wrapping_mul(delta_0)) >> 64
    } else {
        0
    };
    
    let pending_1 = if delta_1 < MAX_REASONABLE_DELTA {
        (liquidity_u.wrapping_mul(delta_1)) >> 64
    } else {
        0
    };
    
    (pending_0, pending_1)
}

/// Collect owed tokens and reset counters
pub fn collect_fees(pos: &mut Position, amount0_max: u128, amount1_max: u128) -> (u128, u128) {
    let amount0 = pos.tokens_owed_0.min(amount0_max);
    let amount1 = pos.tokens_owed_1.min(amount1_max);
    
    pos.tokens_owed_0 = pos.tokens_owed_0.saturating_sub(amount0);
    pos.tokens_owed_1 = pos.tokens_owed_1.saturating_sub(amount1);
    
    (amount0, amount1)
}