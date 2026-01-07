use soroban_sdk::{Address, Env};

use crate::storage::{read_position as storage_read, write_position as storage_write};
use crate::types::Position;

// ============================================================
// POSITION STORAGE HELPERS
// ============================================================

/// Read a position from storage
pub fn read_position(env: &Env, owner: &Address, lower: i32, upper: i32) -> Position {
    storage_read(env, owner, lower, upper)
}

/// Write a position to storage
pub fn write_position(env: &Env, owner: &Address, lower: i32, upper: i32, pos: &Position) {
    storage_write(env, owner, lower, upper, pos);
}

// ============================================================
// POSITION UPDATE (Fee Accumulation)
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
/// 
/// # Arguments
/// * `pos` - Mutable reference to position
/// * `fee_growth_inside_0` - Current fee growth inside for token0
/// * `fee_growth_inside_1` - Current fee growth inside for token1
pub fn update_position(
    pos: &mut Position,
    fee_growth_inside_0: u128,
    fee_growth_inside_1: u128,
) {
    if pos.liquidity > 0 {
        let liquidity_u = pos.liquidity as u128;
        
        // Calculate fee deltas using wrapping subtraction
        let delta_0 = fee_growth_inside_0.wrapping_sub(pos.fee_growth_inside_last_0);
        let delta_1 = fee_growth_inside_1.wrapping_sub(pos.fee_growth_inside_last_1);
        
        // Calculate owed fees using checked multiplication to detect overflow
        // fee = (liquidity * delta) >> 64
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

// ============================================================
// POSITION MODIFICATION
// ============================================================

/// Modify a position's liquidity
/// 
/// This combines fee update with liquidity change:
/// 1. First update fees based on current fee_growth_inside
/// 2. Then adjust liquidity
/// 
/// # Arguments
/// * `pos` - Mutable reference to position
/// * `liquidity_delta` - Change in liquidity (positive = add, negative = remove)
/// * `fee_growth_inside_0` - Current fee growth inside for token0
/// * `fee_growth_inside_1` - Current fee growth inside for token1
pub fn modify_position(
    pos: &mut Position,
    liquidity_delta: i128,
    fee_growth_inside_0: u128,
    fee_growth_inside_1: u128,
) {
    // First update fees
    update_position(pos, fee_growth_inside_0, fee_growth_inside_1);
    
    // Then adjust liquidity
    if liquidity_delta > 0 {
        pos.liquidity = pos.liquidity.saturating_add(liquidity_delta);
    } else {
        pos.liquidity = pos.liquidity.saturating_sub(liquidity_delta.abs());
    }
}

// ============================================================
// PENDING FEE CALCULATION
// ============================================================

/// Calculate pending fees without modifying position
/// 
/// This is a read-only calculation for display purposes.
/// Uses the same formula as update_position but doesn't modify state.
/// 
/// # Arguments
/// * `pos` - Reference to position
/// * `fee_growth_inside_0` - Current fee growth inside for token0
/// * `fee_growth_inside_1` - Current fee growth inside for token1
/// 
/// # Returns
/// (pending_fee_0, pending_fee_1)
pub fn calculate_pending_fees(
    pos: &Position,
    fee_growth_inside_0: u128,
    fee_growth_inside_1: u128,
) -> (u128, u128) {
    if pos.liquidity <= 0 {
        return (0, 0);
    }
    
    let liquidity_u = pos.liquidity as u128;
    
    // Calculate deltas
    let delta_0 = fee_growth_inside_0.wrapping_sub(pos.fee_growth_inside_last_0);
    let delta_1 = fee_growth_inside_1.wrapping_sub(pos.fee_growth_inside_last_1);
    
    // Calculate pending fees with overflow protection
    let pending_0 = match liquidity_u.checked_mul(delta_0) {
        Some(product) => product >> 64,
        None => 0,
    };
    
    let pending_1 = match liquidity_u.checked_mul(delta_1) {
        Some(product) => product >> 64,
        None => 0,
    };
    
    (pending_0, pending_1)
}

// ============================================================
// POSITION HELPERS
// ============================================================

/// Check if a position has any liquidity
#[inline]
pub fn has_liquidity(pos: &Position) -> bool {
    pos.liquidity > 0
}

/// Check if a position has uncollected fees
#[inline]
#[allow(dead_code)]
pub fn has_uncollected_fees(pos: &Position) -> bool {
    pos.tokens_owed_0 > 0 || pos.tokens_owed_1 > 0
}

/// Check if a position is empty (no liquidity and no fees)
#[inline]
#[allow(dead_code)]
pub fn is_empty(pos: &Position) -> bool {
    pos.liquidity == 0 && pos.tokens_owed_0 == 0 && pos.tokens_owed_1 == 0
}

/// Clear collected fees from position
#[allow(dead_code)]
pub fn clear_fees(pos: &mut Position, amount0: u128, amount1: u128) {
    pos.tokens_owed_0 = pos.tokens_owed_0.saturating_sub(amount0);
    pos.tokens_owed_1 = pos.tokens_owed_1.saturating_sub(amount1);
}