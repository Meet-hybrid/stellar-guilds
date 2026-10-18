# Contributing to Stellar Guilds

Thanks for helping. This is a small, security-sensitive Soroban contract, so the rules are strict and the review loop is fast.

## Setup

```bash
git clone https://github.com/Meet-hybrid/stellar-guilds.git
cd stellar-guilds
rustup target add wasm32v1-none
cargo test
make check
```

That is the whole setup. There is no database, no environment file, and no external service to configure.

| Tool | Needed for | Install |
|---|---|---|
| Rust 1.84+ | everything | https://rustup.rs |
| `wasm32v1-none` target | building the contract | `rustup target add wasm32v1-none` |
| `stellar-cli` 28.x | `stellar contract build`, deployment | https://developers.stellar.org/docs/tools/developer-tools/cli/install-cli |
| `cargo-llvm-cov` | coverage locally | `cargo install cargo-llvm-cov --locked` |

## Project layout

```
contracts/guild-vault/
├── Cargo.toml
└── src/
    ├── lib.rs      contract entrypoints + storage helpers
    ├── types.rs    constants, storage keys, data types, typed errors
    └── test.rs     the test suite
```

`lib.rs` keeps each entrypoint thin: validate, move tokens, mutate storage, emit an event. Anything with arithmetic or branching is a helper so it can be tested in isolation.

## Checks

`make check` runs exactly what CI runs: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

| Target | Purpose |
|---|---|
| `make check` | the CI gate |
| `make test` | unit tests |
| `make lint` | clippy, warnings denied |
| `make build` | `stellar contract build` |
| `make size` | fails if the wasm exceeds the 60 KiB budget |

Coverage: `cargo llvm-cov --package guild-vault --lib --show-missing-lines`. Current: 99.72% lines on `lib.rs`.

## Rules for contract changes

1. **A new error variant needs a test that triggers it.** Rust cannot assert that an enum variant is unused, so a variant with no test is treated as dead code.
2. **Every rejection test must assert the outside world did not change.** It is not enough to assert `Err(AlreadyPaid)`; also assert the balances and counters are untouched. A test that only checks the error code cannot catch a state-mutating failure path.
3. **Boundary tests where a boundary exists.** For the deposit window, test the last allowed ledger and the first rejected one.
4. **Money moves before state is written**, and a failed transfer must leave no trace. Do not reorder this.
5. **No `panic!` in the contract.** Return a typed error. Panic strings cannot be matched by a frontend and are not part of the interface.
6. **Document every public function and every storage key.**
7. **Do not weaken a test to make a change pass.** If an assertion is wrong, say so in the PR with the reasoning; do not delete the assertion.

## Style

- `cargo fmt` decides formatting; do not hand-format.
- Clippy runs with `-D warnings`. The one crate-level allowance (`clippy::too_many_arguments`) exists because the SDK-generated client wrapper of the seven-parameter `initialize` trips the limit on macro-generated code. Adding another allowance needs a justification in the PR description.
- Comments explain *why*. The code already says *what*.

## Picking up work

There is no paid bounty programme. Issues carry a complexity label purely so you can pick work that fits the time you have; treat it as an estimate, not a contract.

### Complexity labels

| Label | Typical work |
|---|---|
| `complexity: trivial` | docs, a viewer, a script, a missing test for existing behaviour |
| `complexity: medium` | one contract function with tests, one script, one small integration |
| `complexity: high` | a feature slice across contract + client + docs |

### Workflow

1. **Find an issue** labelled `status: available`. Read it fully, including its "Out of scope" section.
2. **Claim it** by commenting on the issue. A maintainer applies `status: in-progress`. One open claimed issue per contributor at a time.
3. **Branch from `main`**: `git checkout -b feat/123-short-slug`.
4. **Implement against the acceptance criteria** in the issue. They are a checklist, not a suggestion.
5. **Run `make check`** and paste the output in the PR description.
6. **Open a PR** referencing the issue. The PR template asks for the acceptance criteria with checkboxes ticked.
7. **Respond to review.** Maintainers aim to review within a few days.
8. **Merge** squashes into `main` with the issue number in the commit subject, so verification is unambiguous.

### What gets a PR closed

- Scope creep. A PR that fixes the issue *and* refactors something unrelated is split; the unrelated part becomes a new issue.
- Acceptance criteria not met, or ticked without being demonstrated.
- A weakened, deleted, or `#[ignore]`d test.
- New `todo!()`, `unimplemented!()`, or commented-out code.
- Public functions without doc comments, or a new error variant with no test.
- Formatting or lint failures. CI must be green before review starts.

## Reporting bugs

Open a bug report with the affected function, the arguments used, the observed result, and the expected result. If the bug can move funds, do not open a public issue — follow [SECURITY.md](SECURITY.md).

## Code of conduct

Participation is covered by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
