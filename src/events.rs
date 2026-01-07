use soroban_sdk::{Env, Symbol};

// ============================================================
// EVENT NAMES
// ============================================================

/// Event name constants
pub struct EventName;

impl EventName {
    pub fn initialized(env: &Env) -> Symbol {
        Symbol::new(env, "initialized")
    }
    
    pub fn pool_init(env: &Env) -> Symbol {
        Symbol::new(env, "pool_init")
    }
    
    pub fn add_liquidity(env: &Env) -> Symbol {
        Symbol::new(env, "add_liq")
    }
    
    pub fn remove_liquidity(env: &Env) -> Symbol {
        Symbol::new(env, "remove_liq")
    }
    
    pub fn swap(env: &Env) -> Symbol {
        Symbol::new(env, "swap")
    }
    
    pub fn collect(env: &Env) -> Symbol {
        Symbol::new(env, "collect")
    }
    
    pub fn sync_tick(env: &Env) -> Symbol {
        Symbol::new(env, "synctk")
    }
}

// ============================================================
// EVENT EMITTERS
// ============================================================

/// Emit pool initialized event
pub fn emit_initialized(env: &Env, fee_bps: u32, tick_spacing: i32) {
    env.events().publish(
        (EventName::initialized(env),),
        (fee_bps, tick_spacing),
    );
}

/// Emit pool init event (with price info)
pub fn emit_pool_init(env: &Env, sqrt_price_x64: u128, current_tick: i32, tick_spacing: i32) {
    env.events().publish(
        (EventName::pool_init(env),),
        (sqrt_price_x64, current_tick, tick_spacing),
    );
}

/// Emit add liquidity event
pub fn emit_add_liquidity(env: &Env, liquidity: i128, amount0: i128, amount1: i128) {
    env.events().publish(
        (EventName::add_liquidity(env),),
        (liquidity, amount0, amount1),
    );
}

/// Emit remove liquidity event
pub fn emit_remove_liquidity(env: &Env, liquidity: i128, amount0: i128, amount1: i128) {
    env.events().publish(
        (EventName::remove_liquidity(env),),
        (liquidity, amount0, amount1),
    );
}

/// Emit swap event
pub fn emit_swap(env: &Env, amount_in: i128, amount_out: i128, zero_for_one: bool) {
    env.events().publish(
        (EventName::swap(env),),
        (amount_in, amount_out, zero_for_one),
    );
}

/// Emit collect fees event
pub fn emit_collect(env: &Env, amount0: u128, amount1: u128) {
    env.events().publish(
        (EventName::collect(env),),
        (amount0, amount1),
    );
}

/// Emit tick sync event (for debugging/indexing)
pub fn emit_sync_tick(env: &Env, tick: i32, sqrt_price_x64: u128) {
    env.events().publish(
        (EventName::sync_tick(env),),
        (tick, sqrt_price_x64),
    );
}
