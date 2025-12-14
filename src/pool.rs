use soroban_sdk::{Env, Symbol, contracttype, Address};

use crate::DataKey;
use crate::math::{
    tick_to_sqrt_price_x64,
    snap_tick_to_spacing,
};

// ===========================
// POOL STATE (DATA DINAMIS)
// ===========================
#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolState {
    pub sqrt_price_x64: u128,
    pub current_tick: i32,
    pub liquidity: i128,
    pub tick_spacing: i32,
    pub token0: Address,
    pub token1: Address,

    // GLOBAL FEE ACCUMULATORS
    pub fee_growth_global_a: u128, 
    pub fee_growth_global_b: u128,
}

// Helper State
pub fn read_pool_state(env: &Env) -> PoolState {
    env.storage()
        .persistent()
        .get::<_, PoolState>(&DataKey::PoolState)
        .expect("pool not initialized")
}

pub fn write_pool_state(env: &Env, state: &PoolState) {
    env.storage()
        .persistent()
        .set::<_, PoolState>(&DataKey::PoolState, state);
}

// ===========================
// POOL CONFIG (DATA STATIS)
// ===========================
#[contracttype]
#[derive(Clone)]
pub struct PoolConfig {
    pub admin: Address,
    pub token_a: Address,
    pub token_b: Address,
    pub fee_bps: u32,
}

// Helper Config (Ini yang tadi hilang)
pub fn read_pool_config(env: &Env) -> PoolConfig {
    env.storage()
        .persistent()
        .get::<_, PoolConfig>(&DataKey::PoolConfig)
        .expect("pool config not set")
}

pub fn write_pool_config(env: &Env, cfg: &PoolConfig) {
    env.storage()
        .persistent()
        .set::<_, PoolConfig>(&DataKey::PoolConfig, cfg);
}

// ===========================
// INITIALIZATION
// ===========================
pub fn init_pool(
    env: &Env,
    _sqrt_price_x64: u128,
    initial_tick: i32,
    tick_spacing: i32,
    token0: Address,
    token1: Address,
) {
    if tick_spacing <= 0 {
        panic!("tick_spacing must be > 0");
    }

    let snapped_tick = snap_tick_to_spacing(initial_tick, tick_spacing);
    let sqrt_price_x64 = tick_to_sqrt_price_x64(env, snapped_tick);

    let state = PoolState {
        sqrt_price_x64,
        current_tick: snapped_tick,
        liquidity: 0,
        tick_spacing,
        token0,
        token1,
        // Start fee growth dari 0
        fee_growth_global_a: 0,
        fee_growth_global_b: 0,
    };

    write_pool_state(env, &state);

    env.events().publish(
        (Symbol::new(env, "init_pool"),),
        (sqrt_price_x64, snapped_tick, tick_spacing),
    );
}