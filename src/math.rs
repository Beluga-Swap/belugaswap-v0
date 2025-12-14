#![allow(dead_code)]
use soroban_sdk::Env;

pub const ONE_X64: u128 = 1u128 << 64;
pub const MIN_TICK: i32 = -887_272;
pub const MAX_TICK: i32 =  887_272;

const SQRT_1_0001_X64: u128 = 18447666387855958016u128;

pub fn snap_tick_to_spacing(tick: i32, spacing: i32) -> i32 {
    if spacing <= 0 { panic!("tick_spacing must be > 0"); }
    let spacing_abs = spacing.abs();
    let rem = tick.rem_euclid(spacing_abs);
    tick - rem
}

// -------------------------------------------------------------
// SAFE MATH Q64.64 (Fix Overflow Issue)
// -------------------------------------------------------------

#[inline]
pub fn mul_q64(a: u128, b: u128) -> u128 {
    let a_lo = a & 0xFFFFFFFFFFFFFFFF;
    let a_hi = a >> 64;
    let b_lo = b & 0xFFFFFFFFFFFFFFFF;
    let b_hi = b >> 64;

    let mul_ll = a_lo * b_lo;
    let mul_lh = a_lo * b_hi;
    let mul_hl = a_hi * b_lo;
    let mul_hh = a_hi * b_hi;

    let mid = mul_lh.saturating_add(mul_hl).saturating_add(mul_ll >> 64);
    let res_hi = mul_hh << 64;
    let res = res_hi.saturating_add(mid);
    
    res

}

#[inline]
pub fn div_q64(a: u128, b: u128) -> u128 {
    if b == 0 { return u128::MAX; }
    
    // 1. Coba cara biasa (Kalau muat)
    if a < (u128::MAX >> 64) {
        return (a << 64) / b;
    }

    // 2. Kalau Overflow, pake teknik Sisa Bagi (q + r)
    // Rumus: (a * 2^64) / b  ===  (q * 2^64) + (r * 2^64 / b)
    let q = a / b;
    let r = a % b;
    
    let q_part = q << 64; // Bagian utuh
    
    // Bagian sisa (r * 2^64 / b)
    // Karena r < b, kita harus hati-hati biar gak overflow lagi
    let r_part = if r < (u128::MAX >> 64) {
        (r << 64) / b
    } else {
        // Kalau sisanya pun masih kegedean, kita scaling down dikit (Lossy tapi presisi > 0)
        // Kita bagi dua-duanya dengan 2^32 biar muat
        let r_scaled = r >> 32;
        let b_scaled = b >> 32;
        if b_scaled == 0 { 
            u128::MAX 
        } else {
            (r_scaled << 64) / b_scaled
        }
    };
    
    q_part.saturating_add(r_part)
}

// =============================================================
// Tick <-> sqrt_price_x64
// =============================================================

fn tick_sqrt_price_inner(tick: i32) -> u128 {
    if tick == 0 { return ONE_X64; }

    let negative = tick < 0;
    let mut exp: i32 = if negative { -tick } else { tick };

    let mut base: u128 = SQRT_1_0001_X64;
    let mut result: u128 = ONE_X64;

    while exp > 0 {
        if (exp & 1) != 0 {
            result = mul_q64(result, base);
        }
        base = mul_q64(base, base);
        exp >>= 1;
    }

    if negative {
        div_q64(ONE_X64, result)
    } else {
        result
    }
}

pub fn get_sqrt_ratio_at_tick(tick: i32) -> u128 {
    if tick < MIN_TICK || tick > MAX_TICK { panic!("tick out of range"); }
    tick_sqrt_price_inner(tick)
}

pub fn tick_to_sqrt_price_x64(_env: &Env, tick: i32) -> u128 {
    get_sqrt_ratio_at_tick(tick)
}

// =============================================================
// LIQUIDITY MATH
// =============================================================

fn i128_to_u128_safe(x: i128) -> u128 {
    if x <= 0 { 0 } else { x as u128 }
}

fn u128_to_i128_saturating(x: u128) -> i128 {
    if x > i128::MAX as u128 { i128::MAX } else { x as i128 }
}

pub fn get_liquidity_for_amount0(
    _env: &Env,
    amount0: i128,
    sqrt_price_lower: u128,
    sqrt_price_upper: u128,
) -> i128 {
    if amount0 <= 0 { return 0; }
    let amt0_u = i128_to_u128_safe(amount0);
    let num = mul_q64(amt0_u, mul_q64(sqrt_price_upper, sqrt_price_lower));
    let denom = sqrt_price_upper.saturating_sub(sqrt_price_lower);
    if denom == 0 { return 0; }
    let liq_u = num / denom * ONE_X64; 
    u128_to_i128_saturating(liq_u)
}

pub fn get_liquidity_for_amount1(
    _env: &Env,
    amount1: i128,
    sqrt_price_lower: u128,
    sqrt_price_upper: u128,
) -> i128 {
    if amount1 <= 0 { return 0; }
    let amt1_u = i128_to_u128_safe(amount1);
    let width = sqrt_price_upper.saturating_sub(sqrt_price_lower);
    if width == 0 { return 0; }
    let liq_u = amt1_u.saturating_mul(ONE_X64) / width;
    u128_to_i128_saturating(liq_u)
}

pub fn get_amounts_for_liquidity(
    _env: &Env,
    liquidity: i128,
    sqrt_price_lower: u128,
    sqrt_price_upper: u128,
    current_sqrt_price: u128,
) -> (i128, i128) {
    if liquidity <= 0 { return (0, 0); }
    let liq_u = i128_to_u128_safe(liquidity);
    
    // Clamp price ke range
    let mut sp = current_sqrt_price;
    if sp < sqrt_price_lower { sp = sqrt_price_lower; }
    if sp > sqrt_price_upper { sp = sqrt_price_upper; }

    let mut amount0_u: u128 = 0;
    if sp < sqrt_price_upper {
        // amount0 = L * (sqrtU - P) / (sqrtU * P)
        let num = mul_q64(liq_u, sqrt_price_upper.saturating_sub(sp));
        let denom = mul_q64(sqrt_price_upper, sp).max(1); 
        amount0_u = div_q64(num, denom); 
    }

    let mut amount1_u: u128 = 0;
    if sp > sqrt_price_lower {
        // amount1 = L * (P - sqrtL)
        amount1_u = mul_q64(liq_u, sp.saturating_sub(sqrt_price_lower));
    }

    (u128_to_i128_saturating(amount0_u), u128_to_i128_saturating(amount1_u))
}


// =============================================================
// COMPUTE SWAP STEP (THE CORE)
// =============================================================

pub fn compute_swap_step(
    _env: &Env,
    sqrt_price_current: u128,
    liquidity: i128,
    amount_remaining: i128,
    zero_for_one: bool,
) -> (u128, i128, i128) {
    let liq_u = i128_to_u128_safe(liquidity);
    if liq_u == 0 || amount_remaining <= 0 {
        return (sqrt_price_current, 0, 0);
    }
    let amount_in = amount_remaining;
    let amt_u = i128_to_u128_safe(amount_in);
    let sp = sqrt_price_current;

    if zero_for_one {
        // --- FIX: LOGIC SWAP TURUN (Harga P NEXT) ---
        // Formula: P_next = (L * P) / (L + Amount * P)
        // Note: Amount * P di sini harus raw multiplication (bukan Q64), 
        // karena L * P juga akan raw. Kita ingin rasio.
        
        // 1. Hitung Denominator: L<<64 + (Amount * P)
        let product = amt_u.saturating_mul(sp); // Amount * P
        let liq_shifted = liq_u << 64;          // L * 2^64
        
        let denom = liq_shifted.saturating_add(product);
        if denom == 0 { return (sp, 0, 0); }

        // 2. Hitung Numerator: L * P
        // Kita pakai div_q64 trik: div_q64(L*P, denom) = (L*P * 2^64) / denom
        // Ini cocok dengan rumus P_next.
        let num_base = liq_u.saturating_mul(sp);
        
        // 3. New Price
        let new_sp = div_q64(num_base, denom);

        // 4. Amount Out (y)
        // dy = L * (P - P_next)  [Dalam Q64]
        let diff = sp.saturating_sub(new_sp);
        let amount_out_u = mul_q64(liq_u, diff);
        let amount_out = u128_to_i128_saturating(amount_out_u);

        (new_sp, amount_in, amount_out)
    } else {
        // --- LOGIC SWAP NAIK (Harga P NEXT) ---
        // P_next = P + Amount / L
        let delta_sp = div_q64(amt_u, liq_u); // Amount * 2^64 / L
        let new_sp = sp.saturating_add(delta_sp);
        
        // Amount Out (x)
        // dx = L * (1/P - 1/P_next)
        let term1 = div_q64(liq_u, sp);
        let term2 = div_q64(liq_u, new_sp);
        let amount_out_u = term1.saturating_sub(term2);
        let amount_out = u128_to_i128_saturating(amount_out_u);
        
        (new_sp, amount_in, amount_out)
    }
}

pub fn compute_swap_step_with_target(
    env: &Env,
    sqrt_price_current: u128,
    liquidity: i128,
    amount_specified: i128,
    zero_for_one: bool,
    sqrt_price_target: u128,
) -> (u128, i128, i128) {
    
    // 1. Hitung Max Step tanpa limit
    let (next_sp, input_max, output_max) = compute_swap_step(env, sqrt_price_current, liquidity, amount_specified, zero_for_one);
    
    // 2. Cek apakah melewati target?
    let reached_target = if zero_for_one {
        next_sp <= sqrt_price_target // Turun: kalau next lebih kecil dari target, berarti lewat
    } else {
        next_sp >= sqrt_price_target // Naik: kalau next lebih besar dari target, berarti lewat
    };

    if reached_target {
        // Kalau lewat, kita harus berhenti PAS di target.
        // Hitung ulang input yang dibutuhkan untuk sampai target.
        let liq_u = i128_to_u128_safe(liquidity);
        
        if zero_for_one {
            // Turun: dx_in (y) = L * (P - P_target) ?? Salah. 
            // Input Token 0 (x) -> Formula: dx = L * (1/P_target - 1/P_curr)
            // Wait, zero_for_one is Token 0 IN. Price goes DOWN.
            // Uniswap V3: Token 0 in -> Price Down.
            // Formula Input (x): L * (1/sqrt_target - 1/sqrt_curr)
            
            // Helper: 1/Target - 1/Curr
            let term1 = div_q64(ONE_X64, sqrt_price_target); // 1/P_target
            let term2 = div_q64(ONE_X64, sqrt_price_current); // 1/P_curr
            let diff_inv = term1.saturating_sub(term2);
            
            let input_needed_u = mul_q64(liq_u, diff_inv); // L * diff
            let input_needed = u128_to_i128_saturating(input_needed_u);

            // Output (y) = L * (P_curr - P_target)
            let diff_price = sqrt_price_current.saturating_sub(sqrt_price_target);
            let output_real_u = mul_q64(liq_u, diff_price);
            let output_real = u128_to_i128_saturating(output_real_u);

            return (sqrt_price_target, input_needed, output_real);
            
        } else {
            // Naik: Token 1 IN (y). Price UP.
            // dy = L * (P_target - P_curr)
            let diff = sqrt_price_target.saturating_sub(sqrt_price_current);
            let input_needed_u = mul_q64(liq_u, diff);
            let input_needed = u128_to_i128_saturating(input_needed_u);

            // Output Token 0 (x) = L * (1/P_curr - 1/P_target)
            let term1 = div_q64(liq_u, sqrt_price_current);
            let term2 = div_q64(liq_u, sqrt_price_target);
            let output_real_u = term1.saturating_sub(term2);
            let output_real = u128_to_i128_saturating(output_real_u);

            return (sqrt_price_target, input_needed, output_real);
        }
    }
    
    // Kalau belum sampai target, return step normal
    (next_sp, input_max, output_max)
}
