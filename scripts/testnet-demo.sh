#!/usr/bin/env bash
# Runs a guild on Stellar testnet with real transactions and records every
# transaction hash as evidence.
#
#   bash scripts/testnet-demo.sh full-cycle   # 4 members, 4 rounds, everyone pays (~45 min)
#   bash scripts/testnet-demo.sh default      # 2 members, one misses a round (~15 min)
#
# Requirements: stellar-cli 28.x, curl, and the wasm32v1-none Rust target.
# Accounts are funded by Friendbot; their keys are kept in the OS keyring
# (--secure-store), never in the repository.
#
# USDC: Circle's testnet USDC cannot be minted on demand, so the script issues
# its own test asset with code USDC from a fresh issuer account and wraps it as
# a Stellar Asset Contract. It has 7 decimals and the same transfer semantics,
# but it is a test token with no value. The evidence file records its issuer.
#
# Output: docs/testnet/<scenario>-<UTC timestamp>.json
set -Eeuo pipefail

SCENARIO="${1:-}"
case "$SCENARIO" in
  full-cycle | default) ;;
  *) echo "usage: $0 full-cycle|default" >&2; exit 2 ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NETWORK=testnet
RPC_URL="https://soroban-testnet.stellar.org"
WASM=target/wasm32v1-none/release/guild_vault.wasm
CONTRIBUTION=1000000000 # 100 USDC (7 decimals)
ROUND_LEDGERS=120       # the contract minimum, about 10 minutes
GRACE_LEDGERS=24        # about 2 minutes
STAMP="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
EVIDENCE="docs/testnet/${SCENARIO}-${STAMP}.json"
PREFIX="sgdemo-${SCENARIO}"
STEPS=()

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
fail() { log "FAILED: $*"; write_evidence "failed" "$*"; exit 1; }

# Any unexpected failure is recorded instead of exiting silently.
trap 'fail "command failed at line $LINENO: $BASH_COMMAND"' ERR

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# record STEP [TX] [DETAIL_JSON]
record() {
  local step tx detail
  step="$(json_escape "$1")"
  tx="${2:-}"
  detail="${3:-}"
  local entry="{\"step\":\"${step}\""
  [[ -n "$tx" ]] && entry+=",\"tx\":\"${tx}\""
  [[ -n "$detail" ]] && entry+=",\"detail\":${detail}"
  STEPS+=("${entry}}")
  log "ok: $1${tx:+ (tx $tx)}"
}

write_evidence() {
  local status="$1" error="${2:-}"
  mkdir -p docs/testnet
  {
    printf '{\n  "scenario": "%s",\n  "network": "%s",\n  "status": "%s",\n' "$SCENARIO" "$NETWORK" "$status"
    [[ -n "$error" ]] && printf '  "error": "%s",\n' "$(json_escape "$error")"
    printf '  "started_at": "%s",\n' "$STAMP"
    printf '  "contract_id": "%s",\n  "token": {"code": "USDC", "issuer": "%s", "sac": "%s", "note": "test asset issued by this script; no value"},\n' \
      "${CONTRACT_ID:-}" "${ISSUER:-}" "${SAC:-}"
    printf '  "wasm_sha256": "%s",\n' "${WASM_HASH:-}"
    printf '  "steps": [\n'
    local i
    for i in "${!STEPS[@]}"; do
      printf '    %s' "${STEPS[$i]}"
      ((i < ${#STEPS[@]} - 1)) && printf ','
      printf '\n'
    done
    printf '  ]\n}\n'
  } >"$EVIDENCE"
  log "evidence written to $EVIDENCE"
}

latest_ledger() {
  curl -fsS -X POST -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger"}' "$RPC_URL" |
    grep -oE '"sequence":[0-9]+' | cut -d: -f2
}

wait_for_ledger() {
  local target="$1" now
  while now="$(latest_ledger)" && ((now < target)); do
    log "waiting for ledger $target (now $now)"
    sleep 15
  done
}

identity() {
  local name="${PREFIX}-$1"
  stellar keys generate "$name" --secure-store --fund --network "$NETWORK" --overwrite >/dev/null 2>&1
  stellar keys address "$name"
}

LAST_TX=""
LAST_OUT=""
# send SOURCE COMMAND... — runs a state-changing stellar command as SOURCE.
# Sets LAST_TX (submitted transaction hash) and LAST_OUT (the command result).
# Results go into variables, not stdout, so they survive (no subshell).
send() {
  local source="$1" out arg
  shift
  local args=() inserted=0
  for arg in "$@"; do
    if [[ "$arg" == "--" && $inserted -eq 0 ]]; then
      args+=(--source "${PREFIX}-${source}" --network "$NETWORK")
      inserted=1
    fi
    args+=("$arg")
  done
  ((inserted)) || args+=(--source "${PREFIX}-${source}" --network "$NETWORK")
  LAST_TX=""
  LAST_OUT=""
  out="$("${args[@]}" 2>&1)" || {
    printf '%s\n' "$out" >&2
    return 1
  }
  # The CLI prints the submitted transaction as an explorer link and as
  # "Signing transaction: <hash>"; other 64-hex values (e.g. wasm hashes) are ignored.
  LAST_TX="$(printf '%s\n' "$out" | grep -oE '/tx/[0-9a-f]{64}|[Ss]igning transaction: [0-9a-f]{64}' |
    grep -oE '[0-9a-f]{64}' | tail -n 1 || true)"
  # The command's result is the last line that is not CLI status output.
  LAST_OUT="$({ printf '%s\n' "$out" | grep -vE '^(ℹ️|🔗|✅|🌎|📦|☁️|⚠️)' || true; } | tail -n 1)"
}

invoke() { local source="$1"; shift; send "$source" stellar contract invoke --id "$CONTRACT_ID" -- "$@"; }
view() { stellar contract invoke --id "$CONTRACT_ID" --source "${PREFIX}-admin" --network "$NETWORK" --send=no -- "$@" 2>/dev/null; }
token_balance() {
  stellar contract invoke --id "$SAC" --source "${PREFIX}-admin" --network "$NETWORK" --send=no -- balance --id "$1" 2>/dev/null | tr -dc '0-9'
}

# expect_error CODE SOURCE ARGS... — the call must fail with contract error CODE
expect_error() {
  local code="$1" source="$2" out
  shift 2
  if out="$(stellar contract invoke --id "$CONTRACT_ID" --source "${PREFIX}-${source}" --network "$NETWORK" -- "$@" 2>&1)"; then
    fail "expected contract error #$code from '$*' but the call succeeded"
  fi
  printf '%s' "$out" | grep -q "Error(Contract, #${code})" ||
    fail "expected contract error #$code from '$*', got: $(printf '%s' "$out" | tail -n 1)"
}

# expect_auth_failure REQUIRED_SIGNER SOURCE ARGS... — the call, sent by SOURCE,
# must be refused because REQUIRED_SIGNER's authorization is missing: either the
# CLI cannot produce that signature, or the network rejects the auth entry.
expect_auth_failure() {
  local signer="$1" source="$2" out
  shift 2
  if out="$(stellar contract invoke --id "$CONTRACT_ID" --source "${PREFIX}-${source}" --network "$NETWORK" -- "$@" 2>&1)"; then
    fail "expected an authorization failure from '$*' but the call succeeded"
  fi
  printf '%s' "$out" | grep -qE "Missing signing key for account ${signer}|Error\(Auth" ||
    fail "expected '$*' to require ${signer}'s authorization, got: $(printf '%s' "$out" | tail -n 1)"
}

round_field() { view get_round | grep -oE "\"$1\":[0-9]+" | cut -d: -f2; }

# ---------------------------------------------------------------------------
# Accounts, test USDC and deployment
# ---------------------------------------------------------------------------
if [[ "$SCENARIO" == full-cycle ]]; then MEMBERS=(m1 m2 m3 m4); else MEMBERS=(d1 d2); fi
SEATS=${#MEMBERS[@]}

log "creating and funding testnet accounts"
ADMIN="$(identity admin)"
TREASURY="$(identity treasury)"
ISSUER="$(identity issuer)"
declare -A ADDR
for m in "${MEMBERS[@]}"; do ADDR[$m]="$(identity "$m")"; done
record "funded fresh testnet accounts with Friendbot" "" \
  "{\"admin\":\"$ADMIN\",\"treasury\":\"$TREASURY\",\"issuer\":\"$ISSUER\"}"

ASSET="USDC:${ISSUER}"
FUNDING=$((CONTRIBUTION * SEATS))
for who in treasury "${MEMBERS[@]}"; do
  send "$who" stellar tx new change-trust --line "$ASSET"
done
for m in "${MEMBERS[@]}"; do
  send issuer stellar tx new payment --destination "${ADDR[$m]}" --asset "$ASSET" --amount "$FUNDING"
done
record "issued test USDC: each member holds exactly $((FUNDING / 10000000)) USDC"

send admin stellar contract asset deploy --asset "$ASSET"
SAC="$LAST_OUT"
record "wrapped the test asset as a Stellar Asset Contract" "$LAST_TX" "{\"sac\":\"$SAC\"}"

stellar contract build >/dev/null 2>&1
WASM_HASH="$(sha256sum "$WASM" | cut -d' ' -f1)"
send admin stellar contract deploy --wasm "$WASM"
CONTRACT_ID="$LAST_OUT"
record "deployed guild-vault" "$LAST_TX" "{\"contract_id\":\"$CONTRACT_ID\",\"wasm_sha256\":\"$WASM_HASH\"}"

invoke admin initialize --admin "$ADMIN" --treasury "$TREASURY" --usdc_token "$SAC" \
  --contribution_amount "$CONTRIBUTION" --round_duration_ledgers "$ROUND_LEDGERS" \
  --grace_period_ledgers "$GRACE_LEDGERS" --total_rounds "$SEATS"
record "initialized: ${SEATS} seats, 100 USDC per round, ${ROUND_LEDGERS}-ledger rounds, ${GRACE_LEDGERS}-ledger grace" "$LAST_TX"

for m in "${MEMBERS[@]}"; do
  invoke "$m" join_guild --member "${ADDR[$m]}"
  record "$m joined (payout position follows join order)" "$LAST_TX"
done

# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------
if [[ "$SCENARIO" == full-cycle ]]; then
  for ((round = 0; round < SEATS; round++)); do
    for m in "${MEMBERS[@]}"; do
      invoke "$m" deposit_contribution --member "${ADDR[$m]}"
      record "round $round: $m deposited 100 USDC" "$LAST_TX"
    done
    if ((round == 0)); then
      expect_error 6 m1 deposit_contribution --member "${ADDR[m1]}"
      record "round 0: a second deposit by m1 was rejected with AlreadyPaid (#6)"
      expect_error 7 admin distribute_payout
      record "round 0: payout before the round deadline was rejected with RoundNotReady (#7)"
    fi
    start="$(round_field start_ledger)"
    wait_for_ledger $((start + ROUND_LEDGERS))
    recipient="${MEMBERS[$round]}"
    before="$(token_balance "${ADDR[$recipient]}")"
    invoke admin distribute_payout
    tx="$LAST_TX"
    after="$(token_balance "${ADDR[$recipient]}")"
    received=$((after - before))
    ((received == 3990000000)) || fail "round $round: $recipient received $received stroops, expected 3990000000"
    record "round $round: payout released exactly 399 USDC to $recipient (fee 1 USDC accrued)" "$tx" \
      "{\"recipient\":\"${ADDR[$recipient]}\",\"received_stroops\":\"$received\"}"
  done

  for m in "${MEMBERS[@]}"; do
    bal="$(token_balance "${ADDR[$m]}")"
    ((bal == 3990000000)) || fail "$m ends with $bal stroops, expected 3990000000"
  done
  record "every member ends with exactly 399 USDC (400 paid in, one 399 payout received)"

  expect_auth_failure "$TREASURY" admin withdraw_treasury_fees
  record "fee withdrawal sent by the admin was refused: it requires the treasury's signature"

  invoke treasury withdraw_treasury_fees
  tx="$LAST_TX"
  tbal="$(token_balance "$TREASURY")"
  cbal="$(token_balance "$CONTRACT_ID")"
  ((tbal == 40000000)) || fail "treasury holds $tbal stroops, expected 40000000"
  ((cbal == 0)) || fail "contract still holds $cbal stroops, expected 0"
  record "treasury withdrew exactly 4 USDC of fees; the contract holds 0" "$tx"
else
  invoke d1 deposit_contribution --member "${ADDR[d1]}"
  record "round 0: d1 deposited 100 USDC; d2 did not" "$LAST_TX"
  start="$(round_field start_ledger)"
  deadline=$((start + ROUND_LEDGERS))
  wait_for_ledger $((deadline + 1))
  expect_error 8 admin distribute_payout
  record "after the deadline but inside grace, payout was rejected with NotAllPaid (#8)"
  expect_error 9 admin handle_default --delinquent_member "${ADDR[d2]}"
  record "inside grace, handle_default was rejected with GraceNotExpired (#9)"
  wait_for_ledger $((deadline + GRACE_LEDGERS + 2))
  invoke admin handle_default --delinquent_member "${ADDR[d2]}"
  record "after grace, anyone could mark d2 as defaulted" "$LAST_TX"
  member="$(view get_member --who "${ADDR[d2]}")"
  printf '%s' "$member" | grep -q '"is_defaulted":true' || fail "d2 is not marked defaulted: $member"
  printf '%s' "$member" | grep -q '"reputation_score":0' || fail "d2 reputation is not 0: $member"
  record "d2 is_defaulted=true, reputation_score=0"
  before="$(token_balance "${ADDR[d1]}")"
  invoke admin distribute_payout
  tx="$LAST_TX"
  received=$(($(token_balance "${ADDR[d1]}") - before))
  ((received == 997500000)) || fail "d1 received $received stroops, expected 997500000"
  record "pro-rata payout: d1 received exactly 99.75 USDC (one contribution minus the 0.25% fee)" "$tx" \
    "{\"received_stroops\":\"$received\"}"
fi

write_evidence "passed"
log "all steps passed"
