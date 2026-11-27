import { addr, deployFromHash, i128, invoke, latestLedger, read, struct, tokenBalance, u32 } from './stellar'
import { GUILD_WASM_HASH } from '../config'

// Mirrors of the contract types (`contracts/guild-vault/src/types.rs`).
// i128 values arrive as bigint, u32 values as number.

export interface GuildConfig {
  admin: string
  treasury: string
  usdc_token: string
  contribution_amount: bigint
  round_duration_ledgers: number
  grace_period_ledgers: number
  total_rounds: number
  collateral_bps: number
  bond_amount: bigint
}

export interface RoundState {
  index: number
  start_ledger: number
  paid_count: number
}

export interface MemberState {
  address: string
  has_paid: boolean
  reputation_score: number
  is_defaulted: boolean
  missed_rounds: number
  received: boolean
  bond: bigint
  escrow: bigint
  debt: bigint
  shortfall: bigint
}

export interface GuildParams {
  contribution: bigint
  roundLedgers: number
  graceLedgers: number
  seats: number
  collateralBps: number
}

export type Phase = 'forming' | 'active' | 'completed'

export interface Guild {
  id: string
  config: GuildConfig
  round: RoundState
  members: MemberState[]
  vault: bigint
  treasury: bigint
  collateral: bigint
  ledger: number
  phase: Phase
  /** Last ledger a contribution is accepted this round. */
  deadline: number
  /** After this ledger an incomplete round can be settled. */
  graceEnd: number
  canSettle: boolean
}

const FEE_BPS = 25n
const BPS = 10_000n

export async function loadGuild(id: string): Promise<Guild> {
  const [config, round, seats, vault, treasury, collateral, ledger] = await Promise.all([
    read<GuildConfig>(id, 'get_config'),
    read<RoundState>(id, 'get_round'),
    read<string[]>(id, 'get_members'),
    read<bigint>(id, 'get_vault_balance'),
    read<bigint>(id, 'get_treasury_balance'),
    read<bigint>(id, 'get_collateral_balance'),
    latestLedger(),
  ])
  const members = await Promise.all(seats.map((a) => read<MemberState>(id, 'get_member', addr(a))))

  const phase: Phase =
    round.index >= config.total_rounds ? 'completed' : round.start_ledger === 0 ? 'forming' : 'active'
  const deadline = round.start_ledger + config.round_duration_ledgers
  const graceEnd = deadline + config.grace_period_ledgers
  const allPaid = round.paid_count === config.total_rounds
  const canSettle = phase === 'active' && ledger >= deadline && (allPaid || ledger > graceEnd)

  return {
    id,
    config,
    round,
    members,
    vault: BigInt(vault),
    treasury: BigInt(treasury),
    collateral: BigInt(collateral),
    ledger,
    phase,
    deadline,
    graceEnd,
    canSettle,
  }
}

// --- Economics (mirrors `distribute_payout`) --------------------------------

export function fee(pool: bigint): bigint {
  return (pool * FEE_BPS) / BPS
}

/** Escrow the contract holds back from the recipient of round `r`. */
export function heldAt(c: bigint, seats: number, bps: number, r: number, bond = c): bigint {
  const dues = (c * BigInt(seats - 1 - r) * BigInt(bps)) / BPS
  return dues > bond ? dues - bond : 0n
}

export interface SeatOutlook {
  round: number
  /** Transferred at payout when everyone pays. */
  paidNow: bigint
  /** Held in escrow and returned round by round. */
  held: bigint
  /** Dues still owed after receiving. */
  dues: bigint
  /** Part of those dues not backed by bond or escrow. */
  unbacked: bigint
}

/** What every seat receives, holds and could leave unbacked. */
export function outlook(c: bigint, seats: number, bps: number): SeatOutlook[] {
  const pool = c * BigInt(seats)
  const net = pool - fee(pool)
  return Array.from({ length: seats }, (_, r) => {
    const held = heldAt(c, seats, bps, r)
    const dues = c * BigInt(seats - 1 - r)
    const backed = c + held
    return { round: r, paidNow: net - held, held, dues, unbacked: dues > backed ? dues - backed : 0n }
  })
}

// --- Actions ----------------------------------------------------------------

export const actions = {
  join: (me: string, id: string) => invoke(me, id, 'join_guild', addr(me)),
  leave: (me: string, id: string) => invoke(me, id, 'leave_guild', addr(me)),
  deposit: (me: string, id: string) => invoke(me, id, 'deposit_contribution', addr(me)),
  settle: (me: string, id: string) => invoke(me, id, 'distribute_payout'),
  flagDefault: (me: string, id: string, who: string) => invoke(me, id, 'handle_default', addr(who)),
  repay: (me: string, id: string) => invoke(me, id, 'repay_debt', addr(me)),
  withdrawCollateral: (me: string, id: string) => invoke(me, id, 'withdraw_collateral', addr(me)),
  withdrawFees: (me: string, id: string) => invoke(me, id, 'withdraw_treasury_fees'),
}

/** Deploy a new guild instance and initialize it. Two wallet signatures. */
export async function createGuild(me: string, treasury: string, token: string, p: GuildParams): Promise<string> {
  const id = await deployFromHash(me, GUILD_WASM_HASH)
  await invoke(
    me,
    id,
    'initialize',
    addr(me),
    addr(treasury),
    addr(token),
    struct({
      contribution_amount: i128(p.contribution),
      round_duration_ledgers: u32(p.roundLedgers),
      grace_period_ledgers: u32(p.graceLedgers),
      total_rounds: u32(p.seats),
      collateral_bps: u32(p.collateralBps),
    }),
  )
  return id
}

export { tokenBalance }

// --- Guilds this browser has opened -----------------------------------------

const RECENT_KEY = 'stellar-guilds:recent'

export function recentGuilds(): string[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]')
  } catch {
    return []
  }
}

export function remember(id: string) {
  try {
    const list = [id, ...recentGuilds().filter((g) => g !== id)].slice(0, 8)
    localStorage.setItem(RECENT_KEY, JSON.stringify(list))
  } catch {
    /* storage unavailable: nothing to remember */
  }
}
