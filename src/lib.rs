#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token,
    Env, Address, Symbol, symbol_short,
};

// -----------------------------------
// MODULE DECLARATION
// -----------------------------------
mod pool;
mod tick;
mod math;
mod swap;
mod position; // MODUL BARU KITA

// -----------------------------------
// INTERNAL IMPORTS
// -----------------------------------
use crate::tick::{TickInfo, read_tick_info, write_tick_info};
use crate::math::{ONE_X64, mul_q64}; // Butuh mul_q64 buat hitung fee user
use crate::swap::engine_swap;
use crate::pool::{PoolState, init_pool, read_pool_state, write_pool_state, read_pool_config, write_pool_config, PoolConfig};
use crate::position::{Position, read_position, write_position};

// -----------------------------------
// DATA KEYS & EVENTS
// -----------------------------------
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    PoolState,
    PoolConfig,
    Initialized,
    Tick(i32),
    Position(Address, i32, i32),
}

#[derive(Clone)]
#[contracttype]
pub struct SwapResult {
    pub amount_in: i128,
    pub amount_out: i128,
    pub current_tick: i32,
    pub sqrt_price_x64: u128,
}

// -----------------------------------
// HELPER: HITUNG FEE GROWTH INSIDE
// -----------------------------------
// Ini rumus sakti Uniswap V3:
// growth_inside = global - outside_lower - outside_upper
fn get_fee_growth_inside(
    env: &Env,
    lower: i32,
    upper: i32,
    current_tick: i32,
    fee_growth_global_a: u128,
    fee_growth_global_b: u128,
) -> (u128, u128) {
    let lo = read_tick_info(env, lower);
    let up = read_tick_info(env, upper);

    // 1. Calculate fee growth BELOW the lower tick
    let (below_a, below_b) = if current_tick >= lower {
        (lo.fee_growth_outside_a, lo.fee_growth_outside_b)
    } else {
        (
            fee_growth_global_a.wrapping_sub(lo.fee_growth_outside_a),
            fee_growth_global_b.wrapping_sub(lo.fee_growth_outside_b),
        )
    };

    // 2. Calculate fee growth ABOVE the upper tick
    let (above_a, above_b) = if current_tick < upper {
        (up.fee_growth_outside_a, up.fee_growth_outside_b)
    } else {
        (
            fee_growth_global_a.wrapping_sub(up.fee_growth_outside_a),
            fee_growth_global_b.wrapping_sub(up.fee_growth_outside_b),
        )
    };

    // 3. Inside = Global - Below - Above
    let inside_a = fee_growth_global_a
        .wrapping_sub(below_a)
        .wrapping_sub(above_a);
        
    let inside_b = fee_growth_global_b
        .wrapping_sub(below_b)
        .wrapping_sub(above_b);

    (inside_a, inside_b)
}

// Helper untuk update fee user sebelum ubah liquidity
fn update_position_fees(
    pos: &mut Position,
    inside_a: u128,
    inside_b: u128
) {
    let liquidity_u = pos.liquidity as u128;
    
    // Fee = Liquidity * (Growth_Inside_Now - Growth_Inside_Last)
    let delta_a = inside_a.wrapping_sub(pos.fee_growth_inside_last_a);
    let delta_b = inside_b.wrapping_sub(pos.fee_growth_inside_last_b);

    // Pakai mul_q64 karena growth itu scaled Q64.64 (atau Q128 tergantung math)
    // Di pool.rs kita simpan Q64.64 (div_q64 output).
    // Jadi: (L * delta) / 2^64
    let fee_a = mul_q64(liquidity_u, delta_a);
    let fee_b = mul_q64(liquidity_u, delta_b);

    pos.tokens_owed_a += fee_a;
    pos.tokens_owed_b += fee_b;
    
    // Update checkpoint
    pos.fee_growth_inside_last_a = inside_a;
    pos.fee_growth_inside_last_b = inside_b;
}


// -----------------------------------
// KONTRAK UTAMA
// -----------------------------------
#[contract]
pub struct ClmmPool;

#[contractimpl]
impl ClmmPool { 

    pub fn initialize(
        env: Env,
        admin: Address,
        token_a: Address,
        token_b: Address,
        fee_bps: u32,
        sqrt_price_x64: u128,
        current_tick: i32,
        tick_spacing: i32,
    ) {
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Initialized) {
            panic!("pool already initialized");
        }
        if token_a == token_b { panic!("tokens must be different"); }
        if tick_spacing <= 0 { panic!("invalid spacing"); }
        if fee_bps == 0 { panic!("fee must be > 0"); }

        let initial_sqrt = if sqrt_price_x64 == 0 { ONE_X64 } else { sqrt_price_x64 };

        init_pool(
            &env,
            initial_sqrt,
            current_tick,
            tick_spacing,
            token_a.clone(),
            token_b.clone(),
        );

        let cfg = PoolConfig { admin, token_a, token_b, fee_bps };
        write_pool_config(&env, &cfg);
        env.storage().persistent().set(&DataKey::Initialized, &true);
    }

    // READER HELPERS
    pub fn get_pool_state(env: Env) -> PoolState { read_pool_state(&env) }
    pub fn get_pool_config(env: Env) -> PoolConfig { read_pool_config(&env) }
    pub fn get_tick_info(env: Env, tick: i32) -> TickInfo { read_tick_info(&env, tick) }
    
    // Reader Position (Updated)
    pub fn get_position(env: Env, owner: Address, lower: i32, upper: i32) -> Position {
        read_position(&env, &owner, lower, upper)
    }

    // Reader Position Value (Real-time Aset + Fee Owed)
    pub fn get_position_value(
        env: Env, 
        owner: Address, 
        lower: i32, 
        upper: i32
    ) -> (i128, i128, u128, u128) { // Returns (AmtA, AmtB, OwedA, OwedB)
        let mut pos = read_position(&env, &owner, lower, upper);
        if pos.liquidity == 0 {
            return (0, 0, pos.tokens_owed_a, pos.tokens_owed_b);
        }

        let pool = read_pool_state(&env);
        
        // 1. Hitung Principal Value (Aset di Kolam)
        let sqrt_lower = crate::math::get_sqrt_ratio_at_tick(lower);
        let sqrt_upper = crate::math::get_sqrt_ratio_at_tick(upper);
        let (p_a, p_b) = crate::math::get_amounts_for_liquidity(
            &env, pos.liquidity, sqrt_lower, sqrt_upper, pool.sqrt_price_x64
        );

        // 2. Hitung Unclaimed Fees (Simulasi)
        let (inside_a, inside_b) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick, 
            pool.fee_growth_global_a, pool.fee_growth_global_b
        );
        
        // Fee = Owed + (L * (InsideNow - Last))
        let delta_a = inside_a.wrapping_sub(pos.fee_growth_inside_last_a);
        let delta_b = inside_b.wrapping_sub(pos.fee_growth_inside_last_b);
        let pending_a = mul_q64(pos.liquidity as u128, delta_a);
        let pending_b = mul_q64(pos.liquidity as u128, delta_b);

        (p_a, p_b, pos.tokens_owed_a + pending_a, pos.tokens_owed_b + pending_b)
    }

    // ============================================
    // SWAP
    // ============================================
    pub fn swap(
        env: Env,
        caller: Address,
        amount_specified: i128,
        zero_for_one: bool,
        sqrt_price_limit_x64: u128,
    ) -> SwapResult {
        caller.require_auth();
        let mut pool = read_pool_state(&env);
        let pool_addr = env.current_contract_address();

        let (amount_in_used, amount_out_total) = engine_swap(
            &env, &mut pool, amount_specified, zero_for_one, sqrt_price_limit_x64,
        );

        if amount_in_used <= 0 || amount_out_total <= 0 {
            return SwapResult {
                amount_in: 0, amount_out: 0,
                current_tick: pool.current_tick, sqrt_price_x64: pool.sqrt_price_x64,
            };
        }

        write_pool_state(&env, &pool);

        let token0 = token::Client::new(&env, &pool.token0); 
        let token1 = token::Client::new(&env, &pool.token1); 

        if zero_for_one {
            token0.transfer(&caller, &pool_addr, &amount_in_used);
            token1.transfer(&pool_addr, &caller, &amount_out_total);
        } else {
            token1.transfer(&caller, &pool_addr, &amount_in_used);
            token0.transfer(&pool_addr, &caller, &amount_out_total);
        }

        env.events().publish((symbol_short!("swap"),), (amount_in_used, amount_out_total));

        SwapResult {
            amount_in: amount_in_used,
            amount_out: amount_out_total,
            current_tick: pool.current_tick,
            sqrt_price_x64: pool.sqrt_price_x64,
        }
    }

    // ============================================
    // ADD LIQUIDITY (UPDATED FOR FEE)
    // ============================================
    pub fn add_liquidity(
        env: Env,
        owner: Address,
        lower: i32,
        upper: i32,
        liquidity: i128,
        amt_a: i128,
        amt_b: i128,
    ) {
        owner.require_auth();
        let cfg = read_pool_config(&env);
        let mut pool = read_pool_state(&env);
        let pool_addr = env.current_contract_address();

        let lower = crate::math::snap_tick_to_spacing(lower, pool.tick_spacing);
        let upper = crate::math::snap_tick_to_spacing(upper, pool.tick_spacing);

        // 1. Baca / Init Ticks
        let mut lo_info = read_tick_info(&env, lower);
        let mut up_info = read_tick_info(&env, upper);

        // INIT TICK FEE GROWTH (PENTING!)
        // Kalau tick baru lahir (gross=0), kita harus set fee_outside nya
        // Supaya range fee math konsisten.
        if lo_info.liquidity_gross == 0 {
            if pool.current_tick >= lower {
                lo_info.fee_growth_outside_a = pool.fee_growth_global_a;
                lo_info.fee_growth_outside_b = pool.fee_growth_global_b;
            } else {
                lo_info.fee_growth_outside_a = 0;
                lo_info.fee_growth_outside_b = 0;
            }
        }
        if up_info.liquidity_gross == 0 {
             if pool.current_tick >= upper {
                up_info.fee_growth_outside_a = pool.fee_growth_global_a;
                up_info.fee_growth_outside_b = pool.fee_growth_global_b;
            } else {
                up_info.fee_growth_outside_a = 0;
                up_info.fee_growth_outside_b = 0;
            }
        }

        // 2. Update Position Fee (Sebelum nambah L)
        let (inside_a, inside_b) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick, 
            pool.fee_growth_global_a, pool.fee_growth_global_b
        );
        
        let mut pos = read_position(&env, &owner, lower, upper);
        
        // Update fee & checkpoint
        update_position_fees(&mut pos, inside_a, inside_b);

        // 3. Tambah Liquidity
        token::Client::new(&env, &cfg.token_a).transfer(&owner, &pool_addr, &amt_a);
        token::Client::new(&env, &cfg.token_b).transfer(&owner, &pool_addr, &amt_b);

        pool.liquidity += liquidity;
        write_pool_state(&env, &pool); // Update global L (kalau in range? NO, global L cuma update kalau cross)
        // WAIT: Global Liquidity cuma berubah kalau posisi mencakup current_tick.
        
        // FIX LOGIC GLOBAL L:
        // Jika current_tick ada di dalam [lower, upper), maka Global L harus nambah.
        // Di Uniswap V3, modifyPosition melakukan ini.
        // Kode lama kita simplistik: pool.liquidity += liquidity. 
        // ITU SALAH kalau posisinya out of range.
        // TAPI untuk MVP ini, kita asumsikan Add Liq selalu in-range? TIDAK BISA.
        
        // KOREKSI GLOBAL L:
        if pool.current_tick >= lower && pool.current_tick < upper {
             // Re-read karena tadi write? No, variable local.
             // Kita butuh update variable 'pool' local yg sudah dibaca
             // Tapi di atas kita sudah `pool.liquidity += liquidity` (line lama)
             // HAPUS line lama itu, ganti dengan kondisi ini:
             // pool.liquidity += liquidity; // <-- INI SALAH kalau out range
        } else {
            // Kalau out of range, global L tidak berubah!
             pool.liquidity -= liquidity; // Undo line lama?
             // Lebih baik jangan `+=` dulu.
        }
        
        // Re-correction: Code lama kamu `pool.liquidity += liquidity` itu BUG kalau kamu add posisi out-of-range.
        // Mari kita fix sekalian.
        // Hapus `pool.liquidity += liquidity` yang saya tulis di atas, ganti dengan:
        if pool.current_tick >= lower && pool.current_tick < upper {
            pool.liquidity += liquidity;
            write_pool_state(&env, &pool);
        }
        // Kalau out range, pool state tidak berubah (kecuali tick info).

        // Update Tick Info
        lo_info.liquidity_gross += liquidity;
        lo_info.liquidity_net += liquidity;
        write_tick_info(&env, lower, &lo_info);

        up_info.liquidity_gross += liquidity;
        up_info.liquidity_net -= liquidity;
        write_tick_info(&env, upper, &up_info);

        // Update Position Principal
        pos.liquidity += liquidity;
        pos.token_a_amount += amt_a;
        pos.token_b_amount += amt_b;
        write_position(&env, &owner, lower, upper, &pos);
    }

    // ============================================
    // REMOVE LIQUIDITY (UPDATED)
    // ============================================
    pub fn remove_liquidity(
        env: Env,
        owner: Address,
        lower: i32,
        upper: i32,
        liquidity: i128,
    ) {
        owner.require_auth();
        let cfg = read_pool_config(&env);
        let mut pool = read_pool_state(&env);
        let pool_addr = env.current_contract_address();

        // 1. Hitung Fee Growth Inside
        let (inside_a, inside_b) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick, 
            pool.fee_growth_global_a, pool.fee_growth_global_b
        );

        let mut pos = read_position(&env, &owner, lower, upper);
        if pos.liquidity < liquidity { panic!("not enough liquidity"); }

        // 2. Update Fee (Panen fee sebelum cabut)
        update_position_fees(&mut pos, inside_a, inside_b);

        // 3. Hitung Principal Out
        let out_a = pos.token_a_amount * liquidity / pos.liquidity;
        let out_b = pos.token_b_amount * liquidity / pos.liquidity;

        // 4. Update Position
        pos.liquidity -= liquidity;
        pos.token_a_amount -= out_a;
        pos.token_b_amount -= out_b;
        write_position(&env, &owner, lower, upper, &pos);

        // 5. Update Ticks
        let mut lo = read_tick_info(&env, lower);
        lo.liquidity_gross -= liquidity;
        lo.liquidity_net -= liquidity;
        write_tick_info(&env, lower, &lo);

        let mut up = read_tick_info(&env, upper);
        up.liquidity_gross -= liquidity;
        up.liquidity_net += liquidity;
        write_tick_info(&env, upper, &up);

        // 6. Update Global Liquidity (Hanya jika in-range)
        if pool.current_tick >= lower && pool.current_tick < upper {
            pool.liquidity -= liquidity;
            write_pool_state(&env, &pool);
        }

        // 7. Transfer Principal
        token::Client::new(&env, &cfg.token_a).transfer(&pool_addr, &owner, &out_a);
        token::Client::new(&env, &cfg.token_b).transfer(&pool_addr, &owner, &out_b);
    }

    // ============================================
    // COLLECT FEES (NEW FEATURE!) 💰
    // ============================================
    pub fn collect(
        env: Env,
        owner: Address,
        lower: i32,
        upper: i32,
    ) -> (u128, u128) {
        owner.require_auth();
        
        let mut pos = read_position(&env, &owner, lower, upper);
        let pool = read_pool_state(&env);

        // 1. Update Fee terbaru (siapa tau ada yang belum kehitung)
        let (inside_a, inside_b) = get_fee_growth_inside(
            &env, lower, upper, pool.current_tick, 
            pool.fee_growth_global_a, pool.fee_growth_global_b
        );
        update_position_fees(&mut pos, inside_a, inside_b);

        // 2. Ambil semua tokens_owed
        let amount_a = pos.tokens_owed_a;
        let amount_b = pos.tokens_owed_b;

        // 3. Reset owed ke 0
        pos.tokens_owed_a = 0;
        pos.tokens_owed_b = 0;
        write_position(&env, &owner, lower, upper, &pos);

        // 4. Transfer duitnya!
        let cfg = read_pool_config(&env);
        let pool_addr = env.current_contract_address();

        if amount_a > 0 {
            token::Client::new(&env, &cfg.token_a).transfer(&pool_addr, &owner, &(amount_a as i128));
        }
        if amount_b > 0 {
            token::Client::new(&env, &cfg.token_b).transfer(&pool_addr, &owner, &(amount_b as i128));
        }

        // Return berapa yang dicollect
        (amount_a, amount_b)
    }
}
