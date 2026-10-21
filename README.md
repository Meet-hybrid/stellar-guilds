[![CI](https://github.com/Meet-hybrid/stellar-guilds/actions/workflows/ci.yml/badge.svg)](https://github.com/Meet-hybrid/stellar-guilds/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

# Stellar Guilds

**A trustless rotating savings and credit association (ROSCA) on Soroban.**

A guild is a group that agrees on two numbers: how much each member contributes per round, and how long a round lasts. Every round, members contribute USDC into contract escrow, and the whole pot rotates to one member — in join order, decided before anyone pays. A 0.25% fee on each payout accrues to a treasury address. Nobody ever holds the group's money, and nobody can reorder the payout queue.

In West Africa this is *esusu*, in the Caribbean *partner*, in Latin America *tanda*, in East Africa *chama*. What they have in common is that a human treasurer holds the cash and keeps the ledger. Stellar Guilds removes the human.

---

## Status

This repository is the **contract, its tests and documentation**, with a reproducible testnet deployment. There is no frontend yet; it is listed in the roadmap below.

| Check | Command | Result |
|---|---|---|
| Unit tests | `cargo test` | 34 passing, 0 failing |
| Lints | `cargo clippy --all-targets -- -D warnings` | clean |
| Formatting | `cargo fmt --check` | clean |
| Coverage (`lib.rs`) | `cargo llvm-cov --package guild-vault --lib` | 99.72% lines, 97.10% regions |
| Wasm build | `stellar contract build` | ~14.7 KB, 12 exported functions |
| Error variants | grep vs. named tests | 17 of 18 triggered (`MathOverflow` is unreachable — see below) |
| Testnet | `scripts/testnet-demo.sh` | full 4-member cycle and a default case passed with real transactions ([Deployment](#deployment)) |

Numbers are measured, not estimated. The wasm size is given as ~14.7 KB rather than an exact figure because the artifact embeds build paths: the same commit produces 14,694 bytes on one machine and 14,664 bytes in CI. Both are well under the network's 64 KiB contract size limit, and CI enforces a 60 KiB budget.

Where a check could not reach 100%, the reason is stated in [Honest limitations](#honest-limitations).

---

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                 contracts/guild-vault (immutable)            │
│                                                              │
│  instance:   Config · CurrentRound · VaultBalance            │
│              TreasuryBalance                                 │
│  persistent: MemberList (Vec<Address>, join order)           │
│              MemberState(addr)                               │
│                                                              │
│   deposit ──► token::Client::transfer(member → contract)     │
│                 │                                            │
│                 ▼                                            │
│   payout  ──► 99.75% ──► MemberList[round]                   │
│                0.25%  ──► TreasuryBalance (accrued)          │
│                                    │                         │
│   withdraw_treasury_fees ──────────┘──► treasury (auth)      │
└──────────────────────────────┬───────────────────────────────┘
                               │
                 ┌─────────────▼──────────────┐
                 │  USDC Stellar Asset        │
                 │  Contract (SAC)            │
                 └────────────────────────────┘
```

There is no backend, no database, no indexer, and no keeper in this repository. The contract is the entire system.

### Storage

| Key | Tier | Purpose |
|---|---|---|
| `Config` | instance | Admin, treasury, USDC token, contribution, round length, grace, seat count |
| `CurrentRound` | instance | Round index, opening ledger, paid count |
| `VaultBalance` | instance | USDC collected for the round in progress |
| `TreasuryBalance` | instance | USDC owed to the treasury, held until withdrawn |
| `MemberList` | persistent | `Vec<Address>` in join order — join order **is** payout order |
| `MemberState(addr)` | persistent | Payment flag, reputation, default flag |

Persistent entries have their TTL extended on every read and write, so an active guild does not fall into archival.

---

## How a cycle works

| Step | Function | Who can call | Notes |
|---|---|---|---|
| 1 | `initialize` | admin | Writes config once. Rejects `admin == treasury`, a non-token `usdc_token`, and out-of-range parameters. |
| 2 | `join_guild` | the member | Open until the first contribution. Capped at `total_rounds` seats, max 50. |
| 3 | `deposit_contribution` | the member | First contribution opens round 0 and freezes the member list. Requires every seat to be taken. |
| 4 | `distribute_payout` | **anyone** | Settles the round once the deadline passes. Pays `MemberList[round]` the pool minus fee. |
| 5 | `handle_default` | **anyone** | After the grace period, records the missed contribution and slashes reputation to 0. |
| 6 | `withdraw_treasury_fees` | treasury | Sends every accrued fee to the treasury address. |

Worked example — a 4-member guild, 100 USDC each, 120-ledger rounds:

| Ledger | Event | Vault | Treasury claim |
|---|---|---|---|
| 1000 | Members A–D each contribute 100 | 400 USDC | 0 |
| 1000 + 120 | A receives `400 − 1 = 399` USDC | 0 | 1 USDC |
| 1000 + 240 | B receives 399 USDC | 0 | 2 USDC |
| 1000 + 360 | C receives 399 USDC | 0 | 3 USDC |
| 1000 + 480 | D receives 399 USDC; guild completed | 0 | 4 USDC |

If only 3 of 4 members pay, the round settles **pro-rata** after the grace period: the recipient receives `300 − 0.75 = 299.25` USDC, and the member who missed is flagged `is_defaulted` with reputation 0. The round always advances — a single defaulter cannot freeze the group.

> **"Automated" payout needs a caller.** Soroban has no scheduled execution. `distribute_payout` is permissionless: any member, any script, or any observer can trigger it once the round window has elapsed. The contract does not run itself, and this README will not pretend otherwise.

---

## Fee model

```
pool = contribution × members_who_paid
fee  = pool × 25 / 10_000          // integer division: rounds down, favouring members
net  = pool − fee
```

USDC has 7 decimals, so `1 USDC = 10_000_000` stroops.

| Case | Pool | Fee | Recipient receives |
|---|---|---|---|
| Full round, 4 × 100 USDC | 400 USDC | 1 USDC | **399 USDC** |
| One default, 3 × 100 USDC | 300 USDC | 0.75 USDC | **299.25 USDC** |
| Minimum guild, 2 × 0.1 USDC | 0.2 USDC | 5,000 stroops | 0.1995 USDC |

The fee can never round to zero: with the minimum contribution (0.1 USDC) and two payers it is still 5,000 stroops. A test asserts that floor. All arithmetic uses `checked_*` operations, and the release profile enables `overflow-checks`.

---

## Trust assumptions

What the admin **cannot** do:

- Move principal. There is no admin withdrawal path.
- Choose who gets paid. `distribute_payout` takes no recipient argument — it reads `MemberList[round]`.
- Redirect fees. `withdraw_treasury_fees` takes no destination argument — it pays the treasury address fixed at `initialize`, and requires that address's auth.
- Upgrade the contract. There is no upgrade entrypoint; v1 is immutable.
- Change the schedule or the contribution after `initialize`.

What the admin can do: nothing at all in v1 except pay for the deployment. `initialize` records an admin address and requires its auth once.

### Invariants

1. `token.balance(contract) == VaultBalance + TreasuryBalance` at rest.
2. `VaultBalance == Σ deposits − Σ payouts`.
3. `TreasuryBalance == Σ fees − Σ withdrawals`, never negative.
4. A member is the round recipient at most once per cycle.
5. USDC can only leave to a rotation recipient or the treasury.
6. `paid_count ≤ member_count ≤ 50`.
7. A failed token transfer leaves no state change — the whole invocation reverts.

## Honest limitations

These are real and are not hidden. A reviewer should find them written down, not discover them.

1. **The recipient of a short round absorbs the shortfall.** If 3 of 4 members pay, the round's recipient gets 299.25 USDC instead of 399. No debt is carried forward against the defaulter in v1.
2. **A serial defaulter still gets their turn.** They receive a reduced pool and keep a reputation score of 0. Reputation is informational — no function in the contract reads it, so it is not collateral.
3. **`MAX_MEMBERS = 50` is a hard cap.** Settlement touches every member to reset round flags, so the member list must stay bounded.
4. **The contract is immutable.** A bug means redeployment and migration.
5. **USDC only.** The assumption of no transfer fees and no rebasing is baked in.
6. **`MathOverflow` has no triggering test.** It is a defensive guard: the token contract rejects an overflowing transfer before the vault arithmetic is reached. The test asserts what is actually true — the failed call leaves no state change.
7. **Coverage is 99.72% on `lib.rs`, not 100%.** The residue is macro-generated SDK glue. Every hand-written branch and every error return is exercised.

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
cargo test                      # 34 tests
make check                      # fmt + clippy + tests, exactly what CI runs
make build                      # stellar contract build
make size                       # fails if the wasm exceeds the 60 KiB budget
```

> **Do not use `cargo build --target wasm32-unknown-unknown`.** `soroban-sdk` 28 refuses that target on Rust 1.82+ because it enables `reference-types` and `multi-value`, which the Soroban environment does not support.
>
> **`cargo build --target wasm32v1-none` also fails on its own:** the SDK's build script exits with `soroban-sdk requires stellar-cli v25.2.0+ to build a contract`, and setting `SOROBAN_SDK_BUILD_SYSTEM_SUPPORTS_SPEC_SHAKING_V2=1` to bypass it produces an *unshaken* 33,831-byte wasm rather than the optimized 14,694-byte artifact the budget refers to. Build with `stellar contract build`. CI installs the pinned `stellar-cli` 28.0.0 binary and does the same.

---

## Contract interface

```rust
initialize(env, admin, treasury, usdc_token, contribution_amount,
           round_duration_ledgers, grace_period_ledgers, total_rounds) -> Result<(), Error>
join_guild(env, member)                    -> Result<(), Error>
deposit_contribution(env, member)          -> Result<(), Error>
distribute_payout(env)                     -> Result<(), Error>   // permissionless
handle_default(env, delinquent_member)     -> Result<(), Error>   // permissionless
withdraw_treasury_fees(env)                -> Result<(), Error>   // treasury auth
get_config(env)                            -> Result<GuildConfig, Error>
get_round(env)                             -> Result<RoundState, Error>
get_member(env, who)                       -> Option<MemberState>
get_members(env)                           -> Vec<Address>
get_vault_balance(env)                     -> i128
get_treasury_balance(env)                  -> i128
```

Parameter bounds enforced by `initialize`: contribution ≥ 0.1 USDC, 2 ≤ seats ≤ 50, round length 120–1,051,200 ledgers (~10 minutes to ~60 days), grace ≤ 120,960 ledgers (~7 days), `admin != treasury`.

### Errors

Typed `#[contracterror]` values, codes 1–18: `AlreadyInitialized`, `NotInitialized`, `AlreadyMember`, `NotMember`, `GuildFull`, `AlreadyPaid`, `RoundNotReady`, `NotAllPaid`, `GraceNotExpired`, `AlreadyDefaulted`, `NoFeesToWithdraw`, `InvalidParams`, `MathOverflow`, `Completed`, `NotActive`, `DepositWindowClosed`, `GuildNotFull`, `MemberHasPaid`.

Failures never panic with a string: they return a code a client can branch on.

### Events

`#[contractevent]` structs: `init`, `join`, `deposit`, `payout`, `defaulted`, `fees_withdrawn`, `completed`. A client can reconstruct the entire guild history from `getEvents` on the contract ID without an indexer.

---

## Deployment

The contract runs on **Stellar testnet**. `scripts/testnet-demo.sh` deploys a fresh guild, drives it with real transactions, checks every balance and every expected error, and writes each transaction hash to `docs/testnet/`. The wasm deployed is the one `stellar contract build` produces from this repository (SHA-256 `57add7df…b429`).

Testnet has no mintable USDC, so the script issues its own test asset with code `USDC` (7 decimals) and wraps it as a Stellar Asset Contract. It is a test token with no value; its issuer and SAC address are recorded in each evidence file.

| Scenario | Guild contract | Result | Evidence |
|---|---|---|---|
| Full cycle: 4 members, 100 USDC, 4 rounds | [`CAOFBX…F75V`](https://stellar.expert/explorer/testnet/contract/CAOFBX5R5M77QFHEFOO2AJCFSEKU4W3QRYDL622RN4UHAIP6EKGDF75V) | every payout exactly **399 USDC** (rounds: [`a0bf0a58…`](https://stellar.expert/explorer/testnet/tx/a0bf0a58073506df77fb362f9772205e22c8d00c37aed086b95c45e0b3aed3b9), [`ea12f762…`](https://stellar.expert/explorer/testnet/tx/ea12f762a225b8af3f893ab98b4847950bb103a93ac378835899d7600d4b9b1a), [`ac7191d1…`](https://stellar.expert/explorer/testnet/tx/ac7191d1a2e9928e8d9911c8b103e4ae7894354a6a2b213356e0a92fa62ea784), [`2479cb9c…`](https://stellar.expert/explorer/testnet/tx/2479cb9c08b62f93bf0563f289eddab9b6c9f51763062051cb8b0cdee825379e)); every member ends with exactly 399; treasury withdrew exactly 4 USDC ([`4ffde19f…`](https://stellar.expert/explorer/testnet/tx/4ffde19fba01efbc738a42e5171cff9f38fe866f22bada030c1a0519657b3fd2)) and the contract ends at 0. A second deposit (`AlreadyPaid`), an early payout (`RoundNotReady`) and a fee withdrawal without the treasury's signature were all refused. | [`full-cycle-2026-10-21T09-28-12Z.json`](docs/testnet/full-cycle-2026-10-21T09-28-12Z.json) |
| Default: 2 members, one misses round 0 | [`CCBGTL…63RB`](https://stellar.expert/explorer/testnet/contract/CCBGTLDEZPWVIUO6AEX6MVKKP73R4D5UNYVSYKY5276VPDS4HXEX63RB) | inside grace, payout (`NotAllPaid`) and default flagging (`GraceNotExpired`) were refused; after grace anyone flagged the defaulter ([`03f9a514…`](https://stellar.expert/explorer/testnet/tx/03f9a5148318cde7fe13643c280d2c75ea793673bd9494bf918971bae4d201a6), reputation 0); the round settled pro-rata with exactly **99.75 USDC** to the recipient ([`4327fbf1…`](https://stellar.expert/explorer/testnet/tx/4327fbf182e428e5f94aa9176b3254651b45eb3036d4b4d740f2f2f65cb11205)) | [`default-2026-10-21T09-25-17Z.json`](docs/testnet/default-2026-10-21T09-25-17Z.json) |

Reproduce (accounts are funded by Friendbot and their keys stay in the OS keyring):

```bash
bash scripts/testnet-demo.sh default      # about 15 minutes
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

Tracked as issues; not implemented here:

- A TypeScript client package generated from contract bindings
- A web dashboard for creating and monitoring guilds
- A keeper script for permissionless settlement
- SEP-24 anchor cash-in/cash-out module
- Collateral or slashing for defaults (the fairness gap described above)
- Property-based fuzz tests over round sequences
- Upgrade to `soroban-sdk` 29
- Storage-archival integration test against a live network

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, the checks CI runs, how to claim an open issue, and what a mergeable PR looks like. Bug reports and security disclosures: see [SECURITY.md](SECURITY.md).

## License

MIT — see [LICENSE](LICENSE).
