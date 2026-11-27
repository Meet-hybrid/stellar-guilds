[![CI](https://github.com/Meet-hybrid/stellar-guilds/actions/workflows/ci.yml/badge.svg)](https://github.com/Meet-hybrid/stellar-guilds/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

# Stellar Guilds

**A trustless rotating savings and credit association (ROSCA) on Soroban, where a default is paid for instead of just recorded.**

A guild is a group that agrees on a contribution and a round length. Every round, members contribute USDC into contract escrow and the whole pot rotates to one member in seat order. A 0.25% fee on each payout accrues to a treasury address. Nobody ever holds the group's money, and nobody can reorder the payout queue.

In West Africa this is *esusu*, in the Caribbean *partner*, in Latin America *tanda*, in East Africa *chama*. What they have in common is that a human treasurer holds the cash and the group relies on social pressure to stop an early recipient from walking away. Stellar Guilds replaces the treasurer with a contract and the social pressure with collateral: see [Default protection](#default-protection).

---

## Status

This repository contains the **contract**, its tests and documentation, a **web app** (`web/`), and a reproducible testnet deployment.

| Check | Command | Result |
|---|---|---|
| Unit tests | `cargo test` | 43 passing, 0 failing |
| Lints | `cargo clippy --all-targets -- -D warnings` | clean |
| Formatting | `cargo fmt --check` | clean |
| Coverage (`lib.rs`) | `cargo llvm-cov --package guild-vault --lib` | 99.08% lines, 96.77% functions |
| Wasm build | `stellar contract build` | ~24 KB, 16 exported functions |
| Error variants | grep vs. named tests | 21 of 22 triggered (`MathOverflow` is unreachable, see below) |
| Web app | `cd web && npm run build` | type-checks and builds; reads tested against live testnet guilds |
| Testnet | `scripts/testnet-demo.sh` | full cycle, a covered default, and an early recipient walking away, all with real transactions ([Deployment](#deployment)) |

Numbers are measured, not estimated. The wasm size is approximate because the artifact embeds build paths, so the same commit differs by a few dozen bytes between machines. CI enforces a 60 KiB budget.

Where a check could not reach 100%, the reason is stated in [Honest limitations](#honest-limitations).

---

## Default protection

A ROSCA is credit: the first recipient gets the whole pot after paying one contribution. In v1 of this contract an early recipient could stop paying and the only consequence was a reputation score nothing read. v2 makes every default cost the defaulter before it costs anyone else, in three layers.

| Layer | When | What happens |
|---|---|---|
| **Bond** | On `join_guild` | Every member locks one contribution. Refunded by `leave_guild` before the start, or `withdraw_collateral` after the end. |
| **Your own pot backs you** | A member misses a round *before* receiving | The bond covers the missed contribution so the round's recipient gets a full pot. The defaulter moves to the back of the queue. If they miss again with no bond left, the shortfall is recorded as their debt and deducted from their own payout, which is now the last one, and passed to whoever was short-paid. |
| **Escrow** | A member receives the pot | `collateral_bps` of their remaining dues, minus their bond, is held back. Each round they keep paying releases what is no longer needed. A missed round is paid from that escrow, then from the bond. |

`collateral_bps` is fixed when the guild is created. It is the one real trade-off:

| `collateral_bps` | First recipient of a 5 × 100 USDC guild gets now | Held, returned as they pay | Unbacked if they walk away after receiving |
|---|---|---|---|
| 10000 (100%) | 198.75 USDC | 300 USDC | **0** |
| 5000 (50%) | 398.75 USDC | 100 USDC | 200 USDC, recorded as debt |
| 0 | 498.75 USDC | 0 | 300 USDC, recorded as debt |

At 100% no member can ever be left short, and early recipients still get their escrow back round by round as long as they keep paying. Below 100%, any part of a default that bond and escrow cannot cover is recorded on-chain as debt against the defaulter and a claim for the members who were short-paid. `repay_debt` pays those claims directly. The web app shows this table for the exact guild before anyone joins.

What this does **not** do: it cannot give an early recipient uncollateralized credit *and* guarantee nobody is short-changed. No contract can, because there is nothing to seize from someone who has already left. The guild creator chooses where to sit between the two, and the choice is visible to every member.

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                  contracts/guild-vault (immutable)             │
│                                                                │
│  instance:   Config · CurrentRound · VaultBalance              │
│              TreasuryBalance · CollateralBalance               │
│  persistent: MemberList (Vec<Address>, seat order)             │
│              MemberState(addr): paid, received, bond, escrow,  │
│                                 debt, shortfall, reputation    │
│                                                                │
│  join      ──► bond ──────────────────────► CollateralBalance  │
│  deposit   ──► contribution ──────────────► VaultBalance       │
│  payout    ──► missed? escrow → bond ─────► pool               │
│                pool − 0.25% ──► recipient − own debt − escrow  │
│                0.25%        ──► TreasuryBalance                │
│  withdraw_treasury_fees ──────────────────► treasury (auth)    │
└───────────────────────────────┬────────────────────────────────┘
                                │
                  ┌─────────────▼──────────────┐
                  │  USDC Stellar Asset        │
                  │  Contract (SAC)            │
                  └────────────────────────────┘

web/  Vue 3 + TypeScript app: reads by RPC simulation, writes signed with Freighter
```

There is no backend, no database, no indexer, and no keeper. The contract is the system; the web app is a client for it.

### Storage

| Key | Tier | Purpose |
|---|---|---|
| `Config` | instance | Admin, treasury, USDC token, contribution, round length, grace, seats, `collateral_bps`, bond |
| `CurrentRound` | instance | Round index, opening ledger, paid count |
| `VaultBalance` | instance | USDC contributed to the round in progress |
| `TreasuryBalance` | instance | USDC owed to the treasury, held until withdrawn |
| `CollateralBalance` | instance | Every member's bond plus escrow |
| `MemberList` | persistent | `Vec<Address>` in seat order, which is payout order |
| `MemberState(addr)` | persistent | Paid flag, received flag, bond, escrow, debt, shortfall claim, missed rounds, reputation |

Persistent entries have their TTL extended on every read and write, so an active guild does not fall into archival.

---

## How a cycle works

| Step | Function | Who can call | Notes |
|---|---|---|---|
| 1 | `initialize` | admin | Writes config once. Rejects `admin == treasury`, a non-token `usdc_token`, and out-of-range parameters. |
| 2 | `join_guild` / `leave_guild` | the member | Takes a seat and the bond, or gives both back. Open until the first contribution. Max 50 seats. |
| 3 | `deposit_contribution` | the member | The first contribution opens round 0 and freezes the member list. Requires every seat to be taken. |
| 4 | `distribute_payout` | **anyone** | After the deadline (or after grace if someone has not paid): covers missed contributions from collateral, pays the recipient, holds escrow, starts the next round. |
| 5 | `handle_default` | **anyone** | Optional. After grace, flags an unpaid member before settlement does. |
| 6 | `repay_debt` | the member | Pays recorded debt straight to the members who were short-paid. |
| 7 | `withdraw_collateral` | the member | After the final round, returns remaining bond and escrow. |
| — | `withdraw_treasury_fees` | treasury | Sends every accrued fee to the treasury address. |

Worked example: a 4-member guild, 100 USDC each, `collateral_bps = 0`, everyone pays.

| Ledger | Event | Vault | Collateral | Treasury claim |
|---|---|---|---|---|
| 900 | A–D join, each posting a 100 USDC bond | 0 | 400 USDC | 0 |
| 1000 | A–D each contribute 100 | 400 USDC | 400 USDC | 0 |
| 1000 + 120 | A receives `400 − 1 = 399` USDC | 0 | 400 USDC | 1 USDC |
| … | B, C, D each receive 399 USDC | 0 | 400 USDC | 4 USDC |
| end | each member withdraws their 100 USDC bond | 0 | 0 | 4 USDC |

If B misses round 0, B's bond pays B's share, A still receives 399 USDC, and B moves to seat 4.

> **"Automated" payout needs a caller.** Soroban has no scheduled execution. `distribute_payout` is permissionless: any member, the web app's "Settle round" button, or any script can trigger it once the round window has elapsed.

---

## Fee model

```
pool = contribution × members_who_paid + collateral used to cover missed contributions
fee  = pool × 25 / 10_000          // integer division: rounds down, favouring members
net  = pool − fee
paid = net − recipient's own recorded debt − escrow held back
```

USDC has 7 decimals, so `1 USDC = 10_000_000` stroops.

| Case | Pool | Fee | Recipient receives |
|---|---|---|---|
| Full round, 4 × 100 USDC | 400 USDC | 1 USDC | **399 USDC** |
| One default covered by the bond, 3 × 100 + 100 | 400 USDC | 1 USDC | **399 USDC** |
| One default with no collateral left, 3 × 100 USDC | 300 USDC | 0.75 USDC | **299.25 USDC**, plus a 100 USDC claim on the defaulter |
| Minimum guild, 2 × 0.1 USDC | 0.2 USDC | 5,000 stroops | 0.1995 USDC |

The fee can never round to zero: with the minimum contribution (0.1 USDC) and two payers it is still 5,000 stroops. A test asserts that floor. All arithmetic uses `checked_*` operations, and the release profile enables `overflow-checks`.

---

## Trust assumptions

What the admin **cannot** do:

- Move principal. There is no admin withdrawal path.
- Choose who gets paid. `distribute_payout` takes no recipient argument — it reads `MemberList[round]`.
- Redirect fees. `withdraw_treasury_fees` takes no destination argument — it pays the treasury address fixed at `initialize`, and requires that address's auth.
- Upgrade the contract. There is no upgrade entrypoint; each deployment is immutable.
- Change the schedule, the contribution or `collateral_bps` after `initialize`.
- Reorder seats. The only reordering is automatic: a member who misses before receiving moves to the back.
- Touch a member's bond or escrow. Collateral only pays that member's own missed contributions, goes back to them, or settles their own recorded debt.

What the admin can do: nothing at all except pay for the deployment. `initialize` records an admin address and requires its auth once.

### Invariants

1. `token.balance(contract) == VaultBalance + TreasuryBalance + CollateralBalance` at rest. Tests assert this after every step of every default scenario.
2. `CollateralBalance == Σ (bond + escrow)` over all members.
3. `Σ debt == Σ shortfall`: every unit of recorded debt is exactly one member's claim. Tests assert this too.
4. `TreasuryBalance == Σ fees − Σ withdrawals`, never negative.
5. A member is the round recipient at most once per cycle.
6. USDC can only leave to a round recipient, a member's own collateral refund, a short-paid member's claim, or the treasury.
7. `paid_count ≤ member_count ≤ 50`.
8. A failed token transfer leaves no state change: the whole invocation reverts.

## Honest limitations

These are real and are not hidden. A reviewer should find them written down, not discover them.

1. **Below 100% collateral, a walk-away can still leave members short.** The shortfall is recorded as debt and as a claim, but if the defaulter never calls `repay_debt` it is never paid. The guild creator picks this level and the web app shows the exposure per seat before anyone joins. See [Default protection](#default-protection).
2. **Joining needs capital.** A member needs one contribution for the bond before round 1, and at 100% collateral early recipients get most of their pot back only as they keep paying. That is the price of not needing to trust each other.
3. **Reputation is informational.** No function reads it. It is a public record of missed rounds, not a gate.
4. **`MAX_MEMBERS = 50` is a hard cap.** Settlement touches every member to reset flags and release escrow, so the member list must stay bounded.
4. **The contract is immutable.** A bug means redeployment and migration.
5. **USDC only.** The assumption of no transfer fees and no rebasing is baked in.
6. **`MathOverflow` has no triggering test.** It is a defensive guard: the token contract rejects an overflowing transfer before the vault arithmetic is reached. The test asserts what is actually true — the failed call leaves no state change.
7. **Coverage is 99.08% of lines on `lib.rs`, not 100%.** The uncovered lines are a missing-member-list fallback and two defensive branches: a debt repayment with no matching claim, and debt left alongside remaining collateral at withdrawal. Invariant 3 and the order in which collateral is consumed make both unreachable, so no test can trigger them. Every error return is exercised.

---

## Quick start

Prerequisites:

- Rust 1.84+ (`rustc --version`)
- The `wasm32v1-none` target: `rustup target add wasm32v1-none`
- Optional: `stellar-cli` 28.x for `stellar contract build` and invocation
- Optional: `cargo-llvm-cov` for coverage

```bash
git clone https://github.com/Meet-hybrid/stellar-guilds.git
cd stellar-guilds
rustup target add wasm32v1-none
cargo test                      # 43 tests
make check                      # fmt + clippy + tests, exactly what CI runs
make build                      # stellar contract build
make size                       # fails if the wasm exceeds the 60 KiB budget
```

> **Do not use `cargo build --target wasm32-unknown-unknown`.** `soroban-sdk` 28 refuses that target on Rust 1.82+ because it enables `reference-types` and `multi-value`, which the Soroban environment does not support.
>
> **`cargo build --target wasm32v1-none` also fails on its own:** the SDK's build script exits with `soroban-sdk requires stellar-cli v25.2.0+ to build a contract`, and setting `SOROBAN_SDK_BUILD_SYSTEM_SUPPORTS_SPEC_SHAKING_V2=1` to bypass it produces an *unshaken* wasm rather than the optimized artifact the budget refers to. Build with `stellar contract build`. CI installs the pinned `stellar-cli` 28.0.0 binary and does the same.

### Web app

```bash
cd web
npm install
npm run dev        # http://localhost:5173
npm run build      # type-check (vue-tsc) and production build
```

You need the [Freighter](https://www.freighter.app/) wallet switched to **Testnet**, an account funded by Friendbot, a USDC trustline (`USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5`) and test USDC from the [Circle faucet](https://faucet.circle.com/). Without a wallet the guild pages are still readable: reads are RPC simulations and need no signature.

| Page | What it does |
|---|---|
| `/create` | Deploys a fresh instance of the uploaded wasm and calls `initialize`. Shows, live, what each seat is paid, holds and could leave unbacked at the chosen collateral level. |
| `/guild/:id` | Round timeline with deadline and grace, members in payout order with status, bond, escrow and debt, and every action the connected account can take. |

Optional environment variables: `VITE_RPC_URL` and `VITE_GUILD_WASM_HASH` (to point "Create a guild" at a different upload).

---

## Contract interface

```rust
initialize(env, admin, treasury, usdc_token, params: GuildParams) -> Result<(), Error>
join_guild(env, member)                    -> Result<(), Error>   // takes the bond
leave_guild(env, member)                   -> Result<(), Error>   // before start; refunds the bond
deposit_contribution(env, member)          -> Result<(), Error>
distribute_payout(env)                     -> Result<(), Error>   // permissionless
handle_default(env, delinquent_member)     -> Result<(), Error>   // permissionless
repay_debt(env, member)                    -> Result<(), Error>   // pays short-paid members
withdraw_collateral(env, member)           -> Result<(), Error>   // after the final round
withdraw_treasury_fees(env)                -> Result<(), Error>   // treasury auth
get_config(env)                            -> Result<GuildConfig, Error>
get_round(env)                             -> Result<RoundState, Error>
get_member(env, who)                       -> Option<MemberState>
get_members(env)                           -> Vec<Address>
get_vault_balance(env)                     -> i128
get_treasury_balance(env)                  -> i128
get_collateral_balance(env)                -> i128

struct GuildParams {
    contribution_amount: i128,     // stroops; also the bond
    round_duration_ledgers: u32,
    grace_period_ledgers: u32,
    total_rounds: u32,             // = seats
    collateral_bps: u32,           // 0..=10_000
}
```

Parameter bounds enforced by `initialize`: contribution ≥ 0.1 USDC, 2 ≤ seats ≤ 50, round length 120–1,051,200 ledgers (~10 minutes to ~60 days), grace ≤ 120,960 ledgers (~7 days), `collateral_bps ≤ 10_000`, `admin != treasury`.

### Errors

Typed `#[contracterror]` values, codes 1–22: `AlreadyInitialized`, `NotInitialized`, `AlreadyMember`, `NotMember`, `GuildFull`, `AlreadyPaid`, `RoundNotReady`, `NotAllPaid`, `GraceNotExpired`, `AlreadyDefaulted`, `NoFeesToWithdraw`, `InvalidParams`, `MathOverflow`, `Completed`, `NotActive`, `DepositWindowClosed`, `GuildNotFull`, `MemberHasPaid`, `AlreadyStarted`, `NotCompleted`, `NothingToWithdraw`, `NoDebt`.

Failures never panic with a string: they return a code a client can branch on.

### Events

`#[contractevent]` structs: `init`, `join`, `leave`, `deposit`, `payout` (pool, fee, amount paid, debt recovered, escrow held), `defaulted` (amount covered, amount uncovered, whether the member was demoted), `collateral_released`, `debt_settled`, `fees_withdrawn`, `completed`. A client can reconstruct the entire guild history from `getEvents` on the contract ID without an indexer.

---

## Deployment

The contract runs on **Stellar testnet**. `scripts/testnet-demo.sh` deploys a fresh guild, drives it with real transactions, checks every balance and every expected error, and writes each transaction hash to `docs/testnet/`. The wasm deployed is the one `stellar contract build` produces from this repository (SHA-256 `9b15e613…1777`), and it is the same upload the web app's "Create a guild" deploys.

Testnet has no mintable USDC, so the script issues its own test asset with code `USDC` (7 decimals) and wraps it as a Stellar Asset Contract. It is a test token with no value; its issuer and SAC address are recorded in each evidence file. Guilds created from the web app use Circle's testnet USDC instead.

| Scenario | Guild contract | Result | Evidence |
|---|---|---|---|
| **Walk-away**: 3 members, 100% collateral; the first recipient stops paying | [`CBLMFT…4DOJ`](https://stellar.expert/explorer/testnet/contract/CBLMFTAKS64G2CTQRJ2D2GPH6OKO7KUMFDXTQWCTPWCKKLHNJY5Z4DOJ) | w1 received 199.25 USDC with 100 USDC held in escrow ([`4d801885…`](https://stellar.expert/explorer/testnet/tx/4d80188589214b173a87d0b459624d1d218e71ff46d86d5f798bb5eba39d90fd)), then never paid again. w2 and w3 still received full **299.25 USDC** pots, paid from w1's escrow ([`128ce467…`](https://stellar.expert/explorer/testnet/tx/128ce46714008db344e998a842f2e887fb8e1cf148ea3984f49c552c37d725ed)) and then w1's bond ([`15364d56…`](https://stellar.expert/explorer/testnet/tx/15364d5648a0119765765b0de630864160b5e77af3023780d83081bec36b1abb)). Nobody was short-paid; w1's attempt to withdraw collateral was refused (`NothingToWithdraw`). | [`walkaway-2026-11-27T00-24-16Z.json`](docs/testnet/walkaway-2026-11-27T00-24-16Z.json) |
| **Covered default**: 2 members, one never pays | [`CBXQ2N…O62Y`](https://stellar.expert/explorer/testnet/contract/CBXQ2N4WOW7JN35656YMXJTIJRYGBJOVBOYM7C42VXWB646C52VOO62Y) | Inside grace, settlement (`NotAllPaid`) and flagging (`GraceNotExpired`) were refused. After grace, d2's bond covered the missed contribution and d1 received the full pot minus the fee, exactly **199.5 USDC** ([`02420289…`](https://stellar.expert/explorer/testnet/tx/024202896c6910002c28e7afae6cfb41b7c29c02a869b3c1cd0a9309cda1fa1b)). In v1 the same scenario paid 99.75. | [`default-2026-11-27T00-24-16Z.json`](docs/testnet/default-2026-11-27T00-24-16Z.json) |
| **Full cycle**: 4 members, 100 USDC, everyone pays | [`CC4TAG…6EI3`](https://stellar.expert/explorer/testnet/contract/CC4TAGGC2ZGU3HAZAMI57P5XLMOVXO6RGW4VSYZK4BBWWBWNGO2I6EI3) | Four payouts of exactly **399 USDC**; every member then withdrew their 100 USDC bond ([`48a9502d…`](https://stellar.expert/explorer/testnet/tx/48a9502dfe93ca12dc2507a5dd2c995ceac2c3cb4c54ed9f7bc01401636b2ca0)), the treasury withdrew exactly 4 USDC ([`eedd838c…`](https://stellar.expert/explorer/testnet/tx/eedd838c3cff30be2b6eebaee9c3faab0a108850dbdc7868a114739c0af203a2)) and the contract ended at 0. A second deposit, an early payout and a fee withdrawal without the treasury's signature were refused. | [`full-cycle-2026-11-27T00-24-16Z.json`](docs/testnet/full-cycle-2026-11-27T00-24-16Z.json) |

The October 2026 evidence files in `docs/testnet/` are from v1 (no bonds or escrow) and are kept as history.

Reproduce (accounts are funded by Friendbot and their keys stay in the OS keyring):

```bash
bash scripts/testnet-demo.sh default      # about 15 minutes
bash scripts/testnet-demo.sh walkaway     # about 40 minutes
bash scripts/testnet-demo.sh full-cycle   # about 45 minutes: rounds are 120 ledgers, the contract minimum
```

For a real deployment, pass the **Stellar Asset Contract address** of USDC, never the issuer account. Mainnet issuer: `GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN`. Derive the SAC address with:

```bash
stellar contract id asset --asset USDC:GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN --network mainnet
```

`initialize` calls `decimals()` on the token, so passing a `G…` account address fails the invocation instead of silently creating a broken guild.

---

## Fiat on-ramps and SEP-24 (context, not integration)

Most people who use an *esusu* or *chama* do not hold USDC in a Stellar wallet, and the members of a savings circle are exactly the population that needs a cash path. SEP-24 defines an interactive anchor deposit/withdrawal flow on Stellar, and MoneyGram's Stellar-based cash ramps provide the cash-in/cash-out leg in supported markets.

A realistic journey is: a member cashes in local currency to USDC through an anchor's SEP-24 deposit, contributes to the guild contract, and eventually cashes the payout back out. Soroban contracts cannot call out to an anchor, and the anchor's compliance obligations (KYC, travel rule) belong to the anchor, not to a savings contract.

**No SEP-24 or MoneyGram integration exists in this repository.** It is listed as a future module because it determines whether the protocol is usable by the people it is meant for — not because it is built.

---

## Roadmap

Each item is an open issue with scope and acceptance criteria; see the [issue list](https://github.com/Meet-hybrid/stellar-guilds/issues). Highlights:

- A keeper script that settles due rounds, and a "settle all due guilds" mode in the web app
- Property-based tests over random sequences of joins, payments, misses and repayments, checking invariants 1–3
- Guild history in the web app from `getEvents`, and a guild directory
- A TypeScript client package generated from contract bindings
- SEP-24 anchor cash-in/cash-out journey
- Storage-archival integration test against a live network
- `stellar-cli` 29 for builds (the SDK is already on 29)

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, the checks CI runs, how to claim an open issue, and what a mergeable PR looks like. Bug reports and security disclosures: see [SECURITY.md](SECURITY.md).

## License

MIT — see [LICENSE](LICENSE).
