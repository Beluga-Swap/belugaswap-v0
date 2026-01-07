use soroban_sdk::{contracttype, Address, Env};

use crate::types::{PoolConfig, PoolState, Position, TickInfo, TWAPObservation};

// ============================================================
// STORAGE KEYS
// ============================================================

/// All storage keys used in the contract
#[contracttype]
pub enum DataKey {
    /// Pool state (prices, liquidity, fees)
    PoolState,
    /// Pool configuration (tokens, fee settings)
    PoolConfig,
    /// Initialization flag
    Initialized,
    /// Tick data by tick index
    Tick(i32),
    /// Position by (owner, lower_tick, upper_tick)
    Position(Address, i32, i32),
    /// TWAP observation by index
    TWAPObservation(u32),
    /// Newest TWAP observation index
    TWAPNewestIndex,
    /// TWAP initialization flag
    TWAPInitialized,
}

// ============================================================
// INITIALIZATION STORAGE
// ============================================================

/// Check if pool is initialized
pub fn is_initialized(env: &Env) -> bool {
    env.storage().persistent().has(&DataKey::Initialized)
}

/// Mark pool as initialized
pub fn set_initialized(env: &Env) {
    env.storage().persistent().set(&DataKey::Initialized, &true);
}

// ============================================================
// POOL CONFIG STORAGE
// ============================================================

/// Write pool configuration
pub fn write_pool_config(env: &Env, config: &PoolConfig) {
    env.storage().persistent().set(&DataKey::PoolConfig, config);
}

/// Read pool configuration
pub fn read_pool_config(env: &Env) -> PoolConfig {
    env.storage()
        .persistent()
        .get(&DataKey::PoolConfig)
        .unwrap_or_else(|| panic!("pool not initialized"))
}

// ============================================================
// POOL STATE STORAGE
// ============================================================

/// Write pool state
pub fn write_pool_state(env: &Env, state: &PoolState) {
    env.storage().persistent().set(&DataKey::PoolState, state);
}

/// Read pool state
pub fn read_pool_state(env: &Env) -> PoolState {
    env.storage()
        .persistent()
        .get(&DataKey::PoolState)
        .unwrap_or_else(|| panic!("pool not initialized"))
}

/// Initialize pool state
pub fn init_pool_state(
    env: &Env,
    sqrt_price_x64: u128,
    current_tick: i32,
    tick_spacing: i32,
    token0: Address,
    token1: Address,
) {
    let state = PoolState {
        sqrt_price_x64,
        current_tick,
        liquidity: 0,
        tick_spacing,
        token0,
        token1,
        fee_growth_global_0: 0,
        fee_growth_global_1: 0,
        protocol_fees_0: 0,
        protocol_fees_1: 0,
    };
    
    write_pool_state(env, &state);
}

// ============================================================
// TICK STORAGE
// ============================================================

/// Write tick info
pub fn write_tick_info(env: &Env, tick: i32, info: &TickInfo) {
    env.storage().persistent().set(&DataKey::Tick(tick), info);
}

/// Read tick info (returns default if not exists)
pub fn read_tick_info(env: &Env, tick: i32) -> TickInfo {
    env.storage()
        .persistent()
        .get(&DataKey::Tick(tick))
        .unwrap_or_default()
}

// ============================================================
// POSITION STORAGE
// ============================================================

/// Write position
pub fn write_position(env: &Env, owner: &Address, lower: i32, upper: i32, pos: &Position) {
    env.storage()
        .persistent()
        .set(&DataKey::Position(owner.clone(), lower, upper), pos);
}

/// Read position (returns default if not exists)
pub fn read_position(env: &Env, owner: &Address, lower: i32, upper: i32) -> Position {
    env.storage()
        .persistent()
        .get(&DataKey::Position(owner.clone(), lower, upper))
        .unwrap_or_default()
}

// ============================================================
// TWAP STORAGE (Reserved for future use)
// ============================================================

/// Write TWAP observation
#[allow(dead_code)]
pub fn write_twap_observation(env: &Env, index: u32, obs: &TWAPObservation) {
    env.storage()
        .persistent()
        .set(&DataKey::TWAPObservation(index), obs);
}

/// Read TWAP observation (returns default if not exists)
#[allow(dead_code)]
pub fn read_twap_observation(env: &Env, index: u32) -> TWAPObservation {
    env.storage()
        .persistent()
        .get(&DataKey::TWAPObservation(index))
        .unwrap_or_default()
}

/// Set newest TWAP index
#[allow(dead_code)]
pub fn set_twap_newest_index(env: &Env, index: u32) {
    env.storage()
        .persistent()
        .set(&DataKey::TWAPNewestIndex, &index);
}

/// Get newest TWAP index
#[allow(dead_code)]
pub fn get_twap_newest_index(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::TWAPNewestIndex)
        .unwrap_or(0)
}

/// Check if TWAP is initialized
#[allow(dead_code)]
pub fn is_twap_initialized(env: &Env) -> bool {
    env.storage().persistent().has(&DataKey::TWAPInitialized)
}

/// Mark TWAP as initialized
#[allow(dead_code)]
pub fn set_twap_initialized(env: &Env) {
    env.storage()
        .persistent()
        .set(&DataKey::TWAPInitialized, &true);
}