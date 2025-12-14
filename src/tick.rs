use soroban_sdk::{Env, contracttype};

use crate::DataKey;
use crate::math::snap_tick_to_spacing;

#[contracttype]
#[derive(Clone, Debug)]
pub struct TickInfo {
    pub liquidity_gross: i128,
    pub liquidity_net: i128,
    
    // FEE TRACKING
    // Fee growth on the "other side" of this tick relative to current tick
    pub fee_growth_outside_a: u128,
    pub fee_growth_outside_b: u128,
}

pub fn read_tick_info(env: &Env, tick: i32) -> TickInfo {
    env.storage()
        .persistent()
        .get::<_, TickInfo>(&DataKey::Tick(tick))
        .unwrap_or(TickInfo {
            liquidity_gross: 0,
            liquidity_net: 0,
            fee_growth_outside_a: 0,
            fee_growth_outside_b: 0,
        })
}

pub fn write_tick_info(env: &Env, tick: i32, info: &TickInfo) {
    if info.liquidity_gross == 0 && info.liquidity_net == 0 {
        // Hapus kalau kosong buat hemat storage
        env.storage().persistent().remove(&DataKey::Tick(tick));
    } else {
        env.storage()
            .persistent()
            .set::<_, TickInfo>(&DataKey::Tick(tick), info);
    }
}

pub fn find_next_initialized_tick(
    env: &Env,
    current_tick: i32,
    tick_spacing: i32,
    zero_for_one: bool,
) -> i32 {
    if tick_spacing <= 0 { return current_tick; }

    let step = if zero_for_one { -tick_spacing } else { tick_spacing };
    
    // Auto-Snap Logic
    let mut tick = snap_tick_to_spacing(current_tick, tick_spacing);

    // Cek immediate tick jika turun (inclusive boundary)
    if zero_for_one {
        let maybe_info = env.storage().persistent().get::<_, TickInfo>(&DataKey::Tick(tick));
        if let Some(info) = maybe_info {
            if info.liquidity_gross > 0 { return tick; }
        }
    }

    let max_step: i32 = 2000; 
    for _ in 0..max_step {
        tick = tick.saturating_add(step);
        let maybe_info = env.storage().persistent().get::<_, TickInfo>(&DataKey::Tick(tick));
        if let Some(info) = maybe_info {
            if info.liquidity_gross > 0 { return tick; }
        }
    }

    current_tick
}

// UPDATE: cross_tick sekarang butuh Global Fee Growth untuk melakukan "Flipping"
pub fn cross_tick(
    env: &Env, 
    tick: i32, 
    liquidity: &mut i128, 
    fee_growth_global_a: u128,
    fee_growth_global_b: u128,
    zero_for_one: bool
) {
    let mut info = read_tick_info(env, tick);

    // 1. Update Liquidity Net
    if zero_for_one {
        *liquidity -= info.liquidity_net;
    } else {
        *liquidity += info.liquidity_net;
    }

    // 2. FLIP FEE GROWTH
    // Logika V3: Saat menyeberang tick, sisi "luar" berubah jadi sisi "dalam" (sebaliknya).
    // Maka: fee_outside_baru = fee_global_sekarang - fee_outside_lama
    info.fee_growth_outside_a = fee_growth_global_a.wrapping_sub(info.fee_growth_outside_a);
    info.fee_growth_outside_b = fee_growth_global_b.wrapping_sub(info.fee_growth_outside_b);

    write_tick_info(env, tick, &info);
}
