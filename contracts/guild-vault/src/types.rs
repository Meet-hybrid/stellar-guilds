use soroban_sdk::{contracterror, contracttype, Address};

/// Protocol fee taken from every payout, in basis points (0.25%).
pub const FEE_BPS: i128 = 25;
/// Denominator used to interpret [`FEE_BPS`].
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Hard cap on guild size. `distribute_payout` touches every member to reset
/// round flags, so the member list must stay bounded.
pub const MAX_MEMBERS: u32 = 50;
/// Smallest accepted contribution: 0.1 USDC (USDC has 7 decimals).
pub const MIN_CONTRIBUTION: i128 = 1_000_000;

/// Shortest accepted round: 120 ledgers (~10 minutes at 5s per ledger).
pub const MIN_ROUND_LEDGERS: u32 = 120;
/// Longest accepted round: 1_051_200 ledgers (~60 days).
pub const MAX_ROUND_LEDGERS: u32 = 1_051_200;
/// Longest accepted grace period: 120_960 ledgers (~7 days).
pub const MAX_GRACE_LEDGERS: u32 = 120_960;

/// Reputation written when a member joins. Grows by 10 per contribution.
pub const REPUTATION_START: u32 = 0;
/// Reputation gained per on-time contribution.
pub const REPUTATION_PER_DEPOSIT: u32 = 10;
/// Reputation ceiling.
pub const REPUTATION_MAX: u32 = 100;

/// Persistent entries are bumped once they are this close to archival.
pub const STORAGE_THRESHOLD: u32 = 1_000;
/// Persistent entries are bumped to live this many ledgers (~30 days).
pub const STORAGE_EXTEND_TO: u32 = 518_400;

/// Storage keys for the guild vault.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Guild-wide configuration, written once by `initialize`.
    Config,
    /// `Vec<Address>` in join order. Join order is payout order.
    MemberList,
    /// Per-member state.
    MemberState(Address),
    /// Active round cursor.
    CurrentRound,
    /// USDC collected for the round in progress.
    VaultBalance,
    /// USDC owed to the treasury, held in the vault until withdrawn.
    TreasuryBalance,
}

/// Immutable guild configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildConfig {
    pub admin: Address,
    pub treasury: Address,
    /// Stellar Asset Contract address of USDC (never the issuer account).
    pub usdc_token: Address,
    /// Contribution per member per round, in stroops.
    pub contribution_amount: i128,
    /// Round length in ledgers.
    pub round_duration_ledgers: u32,
    /// Extra ledgers granted after the deadline before a round settles pro-rata.
    pub grace_period_ledgers: u32,
    /// Number of members, and therefore the number of payouts in a full cycle.
    pub total_rounds: u32,
}

/// Per-member state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberState {
    pub address: Address,
    /// Whether the member has contributed to the round in progress.
    pub has_paid: bool,
    /// Reliability score: starts at 0, +10 per contribution (cap 100), 0 on default.
    pub reputation_score: u32,
    /// Sticky flag: this member missed a round.
    pub is_defaulted: bool,
}

/// Cursor for the round in progress.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundState {
    /// 0-based round counter; also the payout index.
    pub index: u32,
    /// Ledger at which the current round opened. `0` means no deposit yet.
    pub start_ledger: u32,
    /// Members who have contributed to the current round.
    pub paid_count: u32,
}

/// Typed contract errors. Every variant is reachable and tested.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// `initialize` was called twice.
    AlreadyInitialized = 1,
    /// A stateful call was made before `initialize`.
    NotInitialized = 2,
    /// The address is already a member.
    AlreadyMember = 3,
    /// The address is not a member.
    NotMember = 4,
    /// The guild already has `total_rounds` members.
    GuildFull = 5,
    /// This member already contributed to the current round.
    AlreadyPaid = 6,
    /// The round deadline has not been reached yet.
    RoundNotReady = 7,
    /// The settlement window is closing but members are still unpaid.
    NotAllPaid = 8,
    /// The grace period has not expired yet.
    GraceNotExpired = 9,
    /// This member is already flagged as defaulted.
    AlreadyDefaulted = 10,
    /// No protocol fees are owed to the treasury.
    NoFeesToWithdraw = 11,
    /// An `initialize` bound was violated.
    InvalidParams = 12,
    /// Checked arithmetic failed.
    MathOverflow = 13,
    /// Every round has been settled.
    Completed = 14,
    /// Distribution was attempted before the first contribution.
    NotActive = 15,
    /// The contribution window for this round is closed.
    DepositWindowClosed = 16,
    /// The guild cannot start until every seat is filled.
    GuildNotFull = 17,
    /// The member has already contributed, so cannot be defaulted.
    MemberHasPaid = 18,
}
