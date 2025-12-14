use soroban_sdk::{Env, contracttype, Address};
// Kita butuh DataKey dari lib.rs untuk simpan ke storage
use crate::DataKey;

#[contracttype]
#[derive(Clone, Debug)]
pub struct Position {
    pub liquidity: i128,
    
    // Principal (History Deposit)
    pub token_a_amount: i128,
    pub token_b_amount: i128,

    // --- FEE TRACKING (NEW) ---
    
    // Checkpoint: Nilai Fee Growth Global saat terakhir kali posisi ini di-update.
    // Rumus Fee: (Global_Now - Checkpoint) * Liquidity
    pub fee_growth_inside_last_a: u128,
    pub fee_growth_inside_last_b: u128,

    // Dompet: Fee yang sudah dihitung dan disisihkan untuk user
    pub tokens_owed_a: u128,
    pub tokens_owed_b: u128,
}

// --------------------
// STORAGE HELPERS
// --------------------

pub fn read_position(env: &Env, owner: &Address, lower: i32, upper: i32) -> Position {
    env.storage()
        .persistent()
        .get::<_, Position>(&DataKey::Position(owner.clone(), lower, upper))
        .unwrap_or(Position {
            liquidity: 0,
            token_a_amount: 0,
            token_b_amount: 0,
            // Default fee state 0 (Penting!)
            fee_growth_inside_last_a: 0,
            fee_growth_inside_last_b: 0,
            tokens_owed_a: 0,
            tokens_owed_b: 0,
        })
}

pub fn write_position(
    env: &Env,
    owner: &Address,
    lower: i32,
    upper: i32,
    pos: &Position,
) {
    // Kalau liquidity 0 DAN fee owed 0, baru boleh dihapus.
    // Kalau liquidity 0 tapi masih ada fee nyangkut, JANGAN DIHAPUS (Alice belum collect).
    if pos.liquidity == 0 && pos.tokens_owed_a == 0 && pos.tokens_owed_b == 0 {
        env.storage()
            .persistent()
            .remove(&DataKey::Position(owner.clone(), lower, upper));
    } else {
        env.storage()
            .persistent()
            .set::<_, Position>(&DataKey::Position(owner.clone(), lower, upper), pos);
    }
}
