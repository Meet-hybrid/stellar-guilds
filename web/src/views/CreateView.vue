<script setup lang="ts">
import { StrKey } from '@stellar/stellar-sdk'
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import OutlookTable from '../components/OutlookTable.vue'
import { USDC } from '../config'
import { createGuild, remember } from '../lib/guild'
import { duration, toStroops, usdc } from '../lib/format'
import { account, connect } from '../lib/wallet'

const router = useRouter()

const ROUND_PRESETS = [
  { label: '10 minutes (demo)', ledgers: 120 },
  { label: '1 day', ledgers: 17_280 },
  { label: '1 week', ledgers: 120_960 },
  { label: '30 days', ledgers: 518_400 },
]
const GRACE_PRESETS = [
  { label: '2 minutes (demo)', ledgers: 24 },
  { label: '1 day', ledgers: 17_280 },
  { label: '3 days', ledgers: 51_840 },
  { label: '7 days', ledgers: 120_960 },
]

const amount = ref('100')
const seats = ref(5)
const roundLedgers = ref(17_280)
const graceLedgers = ref(17_280)
const collateralPct = ref(100)
const treasury = ref('')
const token = ref(USDC.sac)
const busy = ref(false)
const error = ref<string | null>(null)

const contribution = computed(() => toStroops(amount.value))
const bps = computed(() => collateralPct.value * 100)
const problems = computed(() => {
  const out: string[] = []
  if (contribution.value === null || contribution.value < 1_000_000n) out.push('Contribution must be at least 0.1 USDC.')
  if (seats.value < 2 || seats.value > 50) out.push('A guild has 2 to 50 seats.')
  if (!StrKey.isValidEd25519PublicKey(treasury.value.trim()) && !StrKey.isValidContract(treasury.value.trim()))
    out.push('Enter a treasury address for the 0.25% fee.')
  else if (treasury.value.trim() === account.value) out.push('The treasury must be a different address from yours.')
  if (!StrKey.isValidContract(token.value.trim())) out.push('The token must be a contract address (C…).')
  return out
})

const cycle = computed(() => duration(roundLedgers.value * seats.value))
const pot = computed(() => (contribution.value ?? 0n) * BigInt(seats.value))

async function submit() {
  if (!account.value) return connect()
  if (problems.value.length || contribution.value === null) return
  busy.value = true
  error.value = null
  try {
    const id = await createGuild(account.value, treasury.value.trim(), token.value.trim(), {
      contribution: contribution.value,
      roundLedgers: roundLedgers.value,
      graceLedgers: graceLedgers.value,
      seats: seats.value,
      collateralBps: bps.value,
    })
    remember(id)
    router.push(`/guild/${id}`)
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <h1>Start a guild</h1>
  <p class="muted">
    You deploy a fresh, immutable guild contract and fix its rules. Nobody, including you, can change them or move the
    members' money afterwards.
  </p>

  <div class="layout">
    <form class="card stack" @submit.prevent="submit">
      <div class="grid two">
        <label>
          Contribution per round (USDC)
          <input v-model="amount" inputmode="decimal" />
          <span class="hint">Also the bond each member posts on joining.</span>
        </label>
        <label>
          Seats
          <input v-model.number="seats" type="number" min="2" max="50" />
          <span class="hint">One payout per seat.</span>
        </label>
        <label>
          Round length
          <select v-model.number="roundLedgers">
            <option v-for="p in ROUND_PRESETS" :key="p.ledgers" :value="p.ledgers">{{ p.label }}</option>
          </select>
        </label>
        <label>
          Grace period
          <select v-model.number="graceLedgers">
            <option v-for="p in GRACE_PRESETS" :key="p.ledgers" :value="p.ledgers">{{ p.label }}</option>
          </select>
          <span class="hint">Extra time before a round can settle without everyone.</span>
        </label>
      </div>

      <label>
        Collateral held after receiving: <strong>{{ collateralPct }}%</strong>
        <input v-model.number="collateralPct" type="range" min="0" max="100" step="5" />
        <span class="hint">
          The share of a recipient's remaining dues kept in escrow and returned round by round as they keep paying. 100%
          means an early recipient who walks away cannot leave anyone short. Lower values give early recipients more cash
          up front but leave part of their dues backed only by recorded debt.
        </span>
      </label>

      <label>
        Treasury address
        <input v-model="treasury" placeholder="G… or C…" spellcheck="false" />
        <span class="hint">Receives the 0.25% fee on each payout. Must be different from your account, and a G… account needs a trustline for the token before fees can be withdrawn.</span>
      </label>

      <details>
        <summary class="small muted">Token</summary>
        <label class="token">
          Token contract
          <input v-model="token" spellcheck="false" />
          <span class="hint">
            Default: Circle testnet USDC. Members need a USDC trustline and can fund it from the
            <a :href="USDC.faucet" target="_blank" rel="noopener">Circle faucet</a>.
          </span>
        </label>
      </details>

      <ul v-if="problems.length && treasury" class="notice error problems">
        <li v-for="p in problems" :key="p">{{ p }}</li>
      </ul>
      <p v-if="error" class="notice error" role="alert">{{ error }}</p>

      <button class="btn primary" :disabled="busy || (!!account && problems.length > 0)">
        {{ !account ? 'Connect wallet to continue' : busy ? 'Deploying… confirm in your wallet' : 'Deploy guild' }}
      </button>
      <p class="hint">Two signatures: one to deploy the contract, one to set its rules.</p>
    </form>

    <aside class="stack">
      <div class="card">
        <h2>At a glance</h2>
        <dl class="kv">
          <dt>Pot per round</dt>
          <dd>{{ usdc(pot) }} USDC</dd>
          <dt>Full cycle</dt>
          <dd>≈ {{ cycle }}</dd>
          <dt>Bond per member</dt>
          <dd>{{ usdc(contribution ?? 0n) }} USDC</dd>
          <dt>Fee</dt>
          <dd>0.25% of each pot</dd>
        </dl>
      </div>
      <div class="card">
        <h2>What each seat gets</h2>
        <p class="muted small">If everyone pays. Held escrow comes back as the recipient keeps paying.</p>
        <OutlookTable v-if="contribution" :contribution="contribution" :seats="seats" :bps="bps" compact />
      </div>
    </aside>
  </div>
</template>

<style scoped>
.layout { display: grid; grid-template-columns: minmax(0, 1.3fr) minmax(0, 1fr); gap: 1rem; align-items: start; margin-top: 1.5rem; }
@media (max-width: 860px) { .layout { grid-template-columns: 1fr; } }
.problems { margin: 0; padding-left: 1.75rem; }
.token { margin-top: 0.75rem; }
</style>
