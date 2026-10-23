# Security Policy

## Reporting a vulnerability

Do **not** open a public issue for anything that can move funds, corrupt guild state, or block a payout.

Report it privately to the maintainer address listed on the repository's GitHub profile, or through GitHub's "Report a vulnerability" button under the Security tab. Include:

- The affected function and the exact arguments used
- The observed behaviour and the expected behaviour
- A minimal reproduction (a failing test is ideal)
- Whether funds can be moved, and whether the issue is reachable in normal use

You will get an acknowledgement within 72 hours. Fixes are released as soon as they are verified; there is no bug bounty programme at this time, and credit will be given in the release notes unless you ask otherwise.

## Supported versions

Only `main` is supported. The contract is immutable once deployed — a fix means a new deployment and a migration, so reports are treated as urgent.

## Trust model

| Actor | Can do | Cannot do |
|---|---|---|
| **Admin** (set at `initialize`) | Nothing after initialization | Move principal, choose a payout recipient, redirect fees, upgrade the contract, change the schedule |
| **Treasury** | Call `withdraw_treasury_fees` to receive accrued fees at its own address | Withdraw more than `TreasuryBalance`, or send funds anywhere but its own address |
| **Any member** | Join, contribute once per round, call `distribute_payout` and `handle_default` | Contribute twice in a round, deposit after the deadline, move another member's funds |
| **Anyone at all** | Settle a round, flag a defaulter | Choose a recipient, change amounts, mint, or upgrade |

`initialize` rejects `admin == treasury` so fee collection and administration cannot share one key by accident.

## What is in scope

- Unauthorized movement of USDC held by the contract
- Paying a payout to an address other than `MemberList[round]`
- Extracting more fees than the 25 bps accrued from real contributions
- Preventing a round from settling when it should settle
- Corrupting `VaultBalance` / `TreasuryBalance` so they no longer match the token balance
- Bypassing the once-per-round contribution limit for a member

## What is out of scope

These are documented design decisions, not vulnerabilities:

- **The recipient of a short round absorbs the shortfall.** Pro-rata settlement is deliberate; the alternative is a round that never settles.
- **A defaulting member still receives their rotation.** Reputation is informational and does not gate anything.
- **Any member, or a stranger, can trigger settlement and default flagging.** This is intentional: a round should never depend on one person's willingness to act.
- **USDC-only assumptions** (no transfer fees, no rebasing). The contract is not a general-purpose token vault.
- **The `MathOverflow` guard is unreachable** through a real token: the token contract rejects an overflowing transfer first. It is retained as defence in depth.
- **Ledger-based scheduling is approximate in wall-clock time.** Round length is measured in ledgers, so it drifts with network ledger close times.

## Operational assumptions

- The treasury key is held responsibly. If it is lost, accrued fees are unrecoverable — the contract has no fee-redirection path by design.
- Members are expected to verify the guild parameters (contribution, round length, seat count, treasury address) before joining. The contract enforces bounds but cannot tell anyone that a 60-day round is a bad idea.
- The contract assumes the Stellar Asset Contract address of USDC is passed, never the issuer account. `initialize` proves the address implements SEP-41 metadata by calling `decimals()` on it.

## Repository security

Changes reach `main` only through controls that a contributor cannot switch off:

- **Branch protection on `main`:** pull requests need an approving review (approvals are dismissed when new commits are pushed), the `Repo hygiene`, `Build and test`, `Coverage gate` and `Security Audit` checks must pass on a branch that is up to date with `main`, review conversations must be resolved, history stays linear, and force-pushes and branch deletion are blocked.
- **CODEOWNERS** covers every file and names CI, scripts, the Makefile, toolchain and Cargo manifests explicitly.
- **Repository hygiene** (`scripts/check-repo-hygiene.sh`, run in CI) rejects editor and devcontainer auto-run configs, font and binary files, npm lifecycle scripts and auto-run task settings.
- **[GuildGuard](docs/GUILDGUARD.md)** scans every pull request for supply-chain malware patterns (hidden payloads, Trojan Source characters, decode-and-execute code, non-registry dependency sources, binaries, build-time code) and merges only pull requests a maintainer approved, that are green, and that scan clean. It reads pull requests as git objects and never runs their code.
- **Security Audit** runs on every pull request and weekly: RustSec advisories (`cargo audit`) and the policy in `deny.toml` (crates.io-only sources, permissive licenses, no wildcard versions or yanked crates).
- CI installs a pinned, checksum-verified `stellar-cli`; workflows run with a read-only token.
- Dependabot opens version and security updates, and secret scanning with push protection is enabled.
