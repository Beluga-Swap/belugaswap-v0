use soroban_sdk::{Env, symbol_short};

use crate::pool::PoolState;
// Tambahkan div_q64 untuk hitung fee growth (fee / liquidity)
use crate::math::{compute_swap_step_with_target, get_sqrt_ratio_at_tick, div_q64};
use crate::tick::{find_next_initialized_tick, cross_tick};

/// ENGINE SWAP
pub fn engine_swap(
    env: &Env,
    pool: &mut PoolState,
    amount_specified: i128,
    zero_for_one: bool,
    sqrt_price_limit_x64: u128,
) -> (i128, i128) {
    if amount_specified <= 0 { return (0, 0); }

    let mut amount_remaining: i128 = amount_specified;
    let mut amount_calculated: i128 = 0;

    let mut sqrt_price: u128 = pool.sqrt_price_x64;
    let mut liquidity: i128 = pool.liquidity;
    let mut current_tick: i32 = pool.current_tick;

    // Hardcode Fee 0.3% (30 BPS)
    // Idealnya diambil dari PoolConfig via lib.rs, tapi biar simple kita taruh sini dulu
    let fee_bps: i128 = 30; 

    if liquidity <= 0 { return (0, 0); }

    let mut iter: u32 = 0;
    while iter < 1024 { 
        iter += 1;
        if amount_remaining <= 0 || liquidity <= 0 { break; }

        let mut sqrt_limit = sqrt_price_limit_x64;
        if sqrt_limit == 0 {
            sqrt_limit = if zero_for_one { 1 } else { u128::MAX - 1 };
        }

        // 1. Cari Next Tick
        let next_tick = find_next_initialized_tick(env, current_tick, pool.tick_spacing, zero_for_one);
        let mut sqrt_target = get_sqrt_ratio_at_tick(next_tick);

        if zero_for_one {
            if sqrt_target < sqrt_limit { sqrt_target = sqrt_limit; }
        } else {
            if sqrt_target > sqrt_limit { sqrt_target = sqrt_limit; }
        }

        // 2. LOGIC FEE: Kurangi amount_remaining dengan Fee
        // amount_avail = amount * (1 - fee)
        // amount_avail = amount * (10000 - 30) / 10000
        let amount_avail = amount_remaining * (10000 - fee_bps) / 10000;

        // 3. Hitung Step dengan Amount yang sudah didiskon
        let (sqrt_next, amount_in, amount_out_step) = if sqrt_price == sqrt_target {
             (sqrt_price, 0, 0)
        } else {
             compute_swap_step_with_target(
                env, sqrt_price, liquidity, amount_avail, zero_for_one, sqrt_target
            )
        };

        // 4. HITUNG FEE YANG DIBAYAR
        // Karena amount_in adalah amount BERSIH yang dipakai swap,
        // kita harus hitung gross-nya.
        // Gross = In / (1 - fee)
        // Fee = Gross - In
        // Simplifikasi: Fee = In * fee / (1 - fee) + 1 (round up)
        let mut step_fee = 0;
        
        // Proteksi jika amount_in == amount_avail (Swap menghabiskan semua sisa)
        if amount_in == amount_avail {
            // Fee adalah sisanya
            step_fee = amount_remaining - amount_in;
        } else {
            // Fee proporsional (pembulatan ke atas)
            // step_fee = amount_in * 30 / 9970
            step_fee = (amount_in * fee_bps) / (10000 - fee_bps) + 1;
        }

        // Update sisa
        amount_remaining -= (amount_in + step_fee);
        amount_calculated += amount_out_step;

        // 5. UPDATE FEE GROWTH GLOBAL
        // Growth += Fee / Liquidity
        // Kita pakai div_q64 (fee * 2^64 / L) biar presisi Q64.64
        if liquidity > 0 {
            let fee_u = if step_fee < 0 { 0 } else { step_fee as u128 };
            let liq_u = liquidity as u128; // Liquidity selalu positif di sini
            
            let growth_delta = div_q64(fee_u, liq_u);

            if zero_for_one {
                // Swap Token 0 -> 1. Input Token 0 (A). Fee dalam Token A.
                pool.fee_growth_global_a = pool.fee_growth_global_a.wrapping_add(growth_delta);
            } else {
                // Swap Token 1 -> 0. Input Token 1 (B). Fee dalam Token B.
                pool.fee_growth_global_b = pool.fee_growth_global_b.wrapping_add(growth_delta);
            }
        }

        let target_reached = sqrt_next == sqrt_target;
        let moving_forward = if zero_for_one { sqrt_target <= sqrt_price } else { sqrt_target >= sqrt_price };
        let at_user_limit = sqrt_price_limit_x64 != 0 && sqrt_target == sqrt_price_limit_x64;

        if target_reached && moving_forward && !at_user_limit {
            sqrt_price = sqrt_target;
            
            // 6. CROSS TICK (Pass Global Fee Growth)
            cross_tick(
                env, 
                next_tick, 
                &mut liquidity, 
                pool.fee_growth_global_a, 
                pool.fee_growth_global_b, 
                zero_for_one
            );

            if zero_for_one {
                current_tick = next_tick - 1; 
            } else {
                current_tick = next_tick;
            }
        } else if sqrt_next != sqrt_price {
            sqrt_price = sqrt_next;
            if amount_remaining <= 0 { break; }
        } else {
            break;
        }
    }

    pool.sqrt_price_x64 = sqrt_price;
    pool.liquidity = liquidity;
    pool.current_tick = current_tick;

    env.events().publish((symbol_short!("synctk"),), (pool.current_tick, pool.sqrt_price_x64));

    // Result adalah total yang dikurangi dari user (termasuk fee)
    (amount_specified - amount_remaining, amount_calculated)
}