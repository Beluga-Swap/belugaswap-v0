use soroban_sdk::{symbol_short, Symbol};

// ============================================================
// ERROR SYMBOLS (For PreviewResult and swap validation)
// ============================================================

/// Error symbols used in PreviewResult.error_message
pub struct ErrorSymbol;

impl ErrorSymbol {
    /// Amount too low
    pub fn amt_low() -> Symbol {
        symbol_short!("AMT_LOW")
    }
    
    /// No liquidity
    pub fn no_liq() -> Symbol {
        symbol_short!("NO_LIQ")
    }
    
    /// Slippage too high
    pub fn slip_hi() -> Symbol {
        symbol_short!("SLIP_HI")
    }
    
    /// Output dust (too small)
    pub fn out_dust() -> Symbol {
        symbol_short!("OUT_DUST")
    }
    
    /// Max slippage exceeded
    pub fn slip_max() -> Symbol {
        symbol_short!("SLIP_MAX")
    }
    
    /// Invalid token
    pub fn bad_token() -> Symbol {
        symbol_short!("BAD_TKN")
    }
    
    /// Same token for input and output
    pub fn same_token() -> Symbol {
        symbol_short!("SAME_TKN")
    }
}

// ============================================================
// ERROR MESSAGES (For panic messages)
// ============================================================

/// Human-readable error messages for panics
pub struct ErrorMsg;

impl ErrorMsg {
    pub const ALREADY_INITIALIZED: &'static str = "pool already initialized";
    pub const INVALID_FEE: &'static str = "invalid fee: must be 1-10000 bps";
    pub const INVALID_PROTOCOL_FEE: &'static str = "invalid protocol fee: must be 0-10000 bps";
    pub const INVALID_TICK_SPACING: &'static str = "invalid tick spacing: must be positive";
    pub const INVALID_TICK_RANGE: &'static str = "invalid tick range: lower must be < upper";
    pub const INVALID_TOKEN: &'static str = "invalid token for this pool";
    pub const SAME_TOKEN: &'static str = "input and output tokens are the same";
    pub const SLIPPAGE_EXCEEDED: &'static str = "slippage tolerance exceeded";
    pub const LIQUIDITY_TOO_LOW: &'static str = "liquidity amount too low";
    pub const INSUFFICIENT_LIQUIDITY: &'static str = "insufficient liquidity in position";
    pub const INVALID_LIQUIDITY_AMOUNT: &'static str = "liquidity amount must be positive";
    pub const SWAP_VALIDATION_FAILED: &'static str = "swap validation failed";
}