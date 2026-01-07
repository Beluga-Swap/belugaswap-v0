use soroban_sdk::{contracttype, Address, Symbol};

// ============================================================
// POOL CONFIGURATION
// ============================================================

/// Pool configuration (immutable after initialization)
#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolConfig {
    /// Admin address with special privileges
    pub admin: Address,
    /// First token address (as provided by user)
    pub token_a: Address,
    /// Second token address (as provided by user)
    pub token_b: Address,
    /// Swap fee in basis points (e.g., 30 = 0.30%)
    pub fee_bps: u32,
    /// Protocol fee in basis points (percentage of LP fees)
    pub protocol_fee_bps: u32,
}

// ============================================================
// POOL STATE
// ============================================================

/// Pool state (mutable, updated on every swap/liquidity change)
#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolState {
    /// Current sqrt price in Q64.64 format
    pub sqrt_price_x64: u128,
    /// Current tick
    pub current_tick: i32,
    /// Currently active liquidity (sum of in-range positions)
    pub liquidity: i128,
    /// Tick spacing for this pool
    pub tick_spacing: i32,
    /// Token0 address (sorted: lower address)
    pub token0: Address,
    /// Token1 address (sorted: higher address)
    pub token1: Address,
    /// Global fee growth for token0 (Q64.64 format)
    pub fee_growth_global_0: u128,
    /// Global fee growth for token1 (Q64.64 format)
    pub fee_growth_global_1: u128,
    /// Accumulated protocol fees for token0
    pub protocol_fees_0: u128,
    /// Accumulated protocol fees for token1
    pub protocol_fees_1: u128,
}

// ============================================================
// TICK INFO
// ============================================================

/// Information stored for each initialized tick
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct TickInfo {
    /// Total liquidity referencing this tick
    pub liquidity_gross: i128,
    /// Net liquidity change when crossing left-to-right
    /// Positive for lower ticks, negative for upper ticks
    pub liquidity_net: i128,
    /// Fee growth outside this tick for token0
    pub fee_growth_outside_0: u128,
    /// Fee growth outside this tick for token1
    pub fee_growth_outside_1: u128,
    /// Whether this tick is initialized
    pub initialized: bool,
}

// ============================================================
// POSITION
// ============================================================

/// LP position data
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct Position {
    /// Liquidity in this position
    pub liquidity: i128,
    /// Fee growth inside at last update for token0
    pub fee_growth_inside_last_0: u128,
    /// Fee growth inside at last update for token1
    pub fee_growth_inside_last_1: u128,
    /// Uncollected fees for token0
    pub tokens_owed_0: u128,
    /// Uncollected fees for token1
    pub tokens_owed_1: u128,
}

// ============================================================
// RETURN TYPES (for contract functions)
// ============================================================

/// Position information returned by get_position
#[contracttype]
#[derive(Clone, Debug)]
pub struct PositionInfo {
    /// Position's liquidity
    pub liquidity: i128,
    /// Current amount of token0 in position
    pub amount0: i128,
    /// Current amount of token1 in position
    pub amount1: i128,
    /// Uncollected fees for token0
    pub fees_owed_0: u128,
    /// Uncollected fees for token1
    pub fees_owed_1: u128,
}

/// Swap result returned by swap functions
#[contracttype]
#[derive(Clone, Debug)]
pub struct SwapResult {
    /// Actual amount of input token used
    pub amount_in: i128,
    /// Actual amount of output token received
    pub amount_out: i128,
    /// Current tick after swap
    pub current_tick: i32,
    /// Current sqrt price after swap
    pub sqrt_price_x64: u128,
}

/// Preview result returned by preview_swap functions
#[contracttype]
#[derive(Clone, Debug)]
pub struct PreviewResult {
    /// Amount of input token that would be used
    pub amount_in_used: i128,
    /// Expected amount of output token
    pub amount_out_expected: i128,
    /// Fee that would be paid
    pub fee_paid: i128,
    /// Price impact in basis points
    pub price_impact_bps: i128,
    /// Whether the swap would succeed
    pub is_valid: bool,
    /// Error message if invalid
    pub error_message: Option<Symbol>,
}

// ============================================================
// TWAP TYPES
// ============================================================

/// TWAP observation data
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct TWAPObservation {
    /// Timestamp of this observation
    pub timestamp: u64,
    /// Cumulative tick * time at this observation
    pub tick_cumulative: i128,
    /// Cumulative 1/liquidity * time at this observation
    pub liquidity_cumulative: u128,
}
