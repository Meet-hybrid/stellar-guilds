<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import OutlookTable from '../components/OutlookTable.vue'
import { USDC } from '../config'
import { actions, loadGuild, remember, tokenBalance, type Guild, type MemberState } from '../lib/guild'
import { duration, explorer, short, usdc } from '../lib/format'
import { account, connect } from '../lib/wallet'

const props = defineProps<{ id: string }>()

const guild = ref<Guild | null>(null)
const loadError = ref<string | null>(null)
const balance = ref<bigint | null>(null)
const busy = ref<string | null>(null)
const result = ref<{ ok: boolean; text: string; hash?: string } | null>(null)
let timer: ReturnType<typeof setInterval> | undefined

async function refresh() {
  try {
    guild.value = await loadGuild(props.id)
    loadError.value = null
    remember(props.id)
    balance.value = account.value ? await tokenBalance(guild.value.config.usdc_token, account.value) : null
  } catch (e) {
    if (!guild.value) loadError.value = (e as Error).message
  }
}

onMounted(() => {
  refresh()
  timer = setInterval(refresh, 10_000)
})
onUnmounted(() => clearInterval(timer))
watch(() => [props.id, account.value], refresh)

const g = computed(() => guild.value)
const c = computed(() => g.value?.config.contribution_amount ?? 0n)
const seatsLeft = computed(() => (g.value ? g.value.config.total_rounds - g.value.members.length : 0))
const me = computed<MemberState | undefined>(() => g.value?.members.find((m) => m.address === account.value))
const mySeat = computed(() => (g.value && me.value ? g.value.members.indexOf(me.value) : -1))
const isTreasury = computed(() => !!g.value && g.value.config.treasury === account.value)
const recipient = computed(() => (g.value && g.value.phase !== 'completed' ? g.value.members[g.value.round.index] : undefined))
const inGrace = computed(() => !!g.value && g.value.ledger > g.value.deadline)
const graceOver = computed(() => !!g.value && g.value.ledger > g.value.graceEnd)

const timeline = computed(() => {
  const x = g.value
  if (!x || x.phase !== 'active') return null
  const total = x.config.round_duration_ledgers
  const elapsed = Math.min(x.ledger - x.round.start_ledger, total)
  const pct = Math.max(0, Math.min(100, (elapsed / total) * 100))
  if (x.ledger <= x.deadline) return { pct, grace: false, text: `${duration(x.deadline - x.ledger)} left to contribute` }
  if (x.ledger <= x.graceEnd) return { pct: 100, grace: true, text: `Grace period: ${duration(x.graceEnd - x.ledger)} left for late payers` }
  return { pct: 100, grace: true, text: 'Round closed. Anyone can settle it now.' }
})

async function run(label: string, fn: () => Promise<{ hash: string }>, success: string) {
  if (!account.value) return connect()
  busy.value = label
  result.value = null
  try {
    const { hash } = await fn()
    result.value = { ok: true, text: success, hash }
    await refresh()
  } catch (e) {
    result.value = { ok: false, text: (e as Error).message }
  } finally {
    busy.value = null
  }
}

const id = () => props.id
const join = () => run('join', () => actions.join(account.value!, id()), `You're in. Your seat is #${(g.value?.members.length ?? 0) + 1}.`)
const leave = () => run('leave', () => actions.leave(account.value!, id()), 'You left the guild and your bond was refunded.')
const deposit = () => run('deposit', () => actions.deposit(account.value!, id()), 'Contribution received for this round.')
const settle = () => run('settle', () => actions.settle(account.value!, id()), 'Round settled and the pot paid out.')
const flag = (who: string) => run(`flag-${who}`, () => actions.flagDefault(account.value!, id(), who), `${short(who)} flagged as defaulted.`)
const repay = () => run('repay', () => actions.repay(account.value!, id()), 'Debt repaid to the members you short-changed.')
const withdraw = () => run('withdraw', () => actions.withdrawCollateral(account.value!, id()), 'Collateral returned to your account.')
const fees = () => run('fees', () => actions.withdrawFees(account.value!, id()), 'Fees sent to the treasury.')

function status(m: MemberState, seat: number): { label: string; tone: string }[] {
  const x = g.value!
  const out: { label: string; tone: string }[] = []
  if (x.phase === 'active' && seat === x.round.index) out.push({ label: 'receives this round', tone: 'accent' })
  if (m.received) out.push({ label: 'received', tone: 'ok' })
  if (x.phase === 'active') out.push(m.has_paid ? { label: 'paid', tone: 'ok' } : { label: graceOver.value ? 'missed' : 'unpaid', tone: graceOver.value ? 'bad' : 'warn' })
  if (m.is_defaulted) out.push({ label: `defaulted ×${m.missed_rounds}`, tone: 'bad' })
  return out
}
</script>

<template>
  <p v-if="loadError" class="notice error">
    Could not load this guild: {{ loadError }}
    <RouterLink to="/">Back</RouterLink>
  </p>
  <p v-else-if="!g" class="muted">Loading guild…</p>

  <template v-else>
    <div class="spread head">
      <div>
        <p class="muted small mono">
          <a :href="explorer.account(g.id)" target="_blank" rel="noopener">{{ short(g.id) }}</a>
        </p>
        <h1>{{ usdc(c) }} USDC × {{ g.config.total_rounds }} seats</h1>
        <div class="row">
          <span class="chip" :class="{ ok: g.phase === 'active', accent: g.phase === 'forming' }">
            {{ g.phase === 'forming' ? `Forming · ${seatsLeft} seat${seatsLeft === 1 ? '' : 's'} open` : g.phase === 'active' ? `Round ${g.round.index + 1} of ${g.config.total_rounds}` : 'Completed' }}
          </span>
          <span class="chip">rounds of ≈ {{ duration(g.config.round_duration_ledgers) }}</span>
          <span class="chip" :class="g.config.collateral_bps === 10000 ? 'ok' : 'warn'">
            {{ g.config.collateral_bps / 100 }}% collateral
          </span>
        </div>
      </div>
      <div v-if="balance !== null" class="stat">
        <div class="label">Your USDC</div>
        <div class="value">{{ usdc(balance) }}</div>
      </div>
    </div>

    <div class="grid stats">
      <div class="stat"><div class="label">Pot per round</div><div class="value">{{ usdc(c * BigInt(g.config.total_rounds)) }}</div></div>
      <div class="stat"><div class="label">Collected this round</div><div class="value">{{ usdc(g.vault) }}</div></div>
      <div class="stat"><div class="label">Bonds + escrow held</div><div class="value">{{ usdc(g.collateral) }}</div></div>
      <div class="stat"><div class="label">Fees accrued</div><div class="value">{{ usdc(g.treasury) }}</div></div>
    </div>

    <p v-if="result" class="notice" :class="result.ok ? 'ok' : 'error'" role="status">
      {{ result.text }}
      <a v-if="result.hash" :href="explorer.tx(result.hash)" target="_blank" rel="noopener">View transaction</a>
    </p>

    <div class="grid two">
      <!-- Round -->
      <section class="card">
        <h2>{{ g.phase === 'forming' ? 'Waiting for members' : g.phase === 'active' ? 'This round' : 'Cycle complete' }}</h2>
        <template v-if="g.phase === 'forming'">
          <p class="muted">
            {{ g.members.length }} of {{ g.config.total_rounds }} seats taken. The first contribution starts round 1; it can
            only be made once every seat is filled.
          </p>
          <div class="progress"><span :style="{ width: `${(g.members.length / g.config.total_rounds) * 100}%` }" /></div>
        </template>
        <template v-else-if="g.phase === 'active' && timeline">
          <p>
            Pot goes to <strong class="mono">{{ recipient ? short(recipient.address) : '—' }}</strong>
            <span v-if="recipient?.address === account" class="chip accent">you</span>
          </p>
          <div class="progress"><span :class="{ grace: timeline.grace }" :style="{ width: `${timeline.pct}%` }" /></div>
          <p class="small muted bar-text">{{ timeline.text }} · {{ g.round.paid_count }}/{{ g.config.total_rounds }} paid</p>
          <button class="btn ghost" :disabled="!g.canSettle || !!busy" @click="settle">
            {{ busy === 'settle' ? 'Settling…' : 'Settle round' }}
          </button>
          <p v-if="!g.canSettle && inGrace" class="hint">Settles once everyone pays or the grace period ends.</p>
          <p v-else-if="g.canSettle" class="hint">Anyone can do this. It pays the recipient and starts the next round.</p>
        </template>
        <p v-else class="muted">Every seat has been paid. Members can withdraw their remaining bond and escrow.</p>
      </section>

      <!-- You -->
      <section class="card" :class="{ accent: !!me }">
        <h2>Your seat</h2>
        <template v-if="!account">
          <p class="muted">Connect a wallet to join or manage your seat.</p>
          <button class="btn primary" @click="connect">Connect wallet</button>
        </template>
        <template v-else-if="me">
          <dl class="kv">
            <dt>Seat</dt><dd>#{{ mySeat + 1 }}{{ me.received ? ' · received' : '' }}</dd>
            <dt>Bond</dt><dd>{{ usdc(me.bond) }}</dd>
            <dt>Escrow</dt><dd>{{ usdc(me.escrow) }}</dd>
            <dt v-if="me.debt > 0n">Debt</dt><dd v-if="me.debt > 0n" class="bad">{{ usdc(me.debt) }}</dd>
            <dt v-if="me.shortfall > 0n">Owed to you</dt><dd v-if="me.shortfall > 0n">{{ usdc(me.shortfall) }}</dd>
            <dt>Reputation</dt><dd>{{ me.reputation_score }}/100</dd>
          </dl>
          <div class="row actions">
            <button v-if="g.phase === 'active' && !me.has_paid && g.ledger <= g.deadline" class="btn primary" :disabled="!!busy" @click="deposit">
              {{ busy === 'deposit' ? 'Paying…' : `Pay ${usdc(c)} USDC` }}
            </button>
            <button v-if="g.phase === 'forming' && g.members.length === g.config.total_rounds && !me.has_paid" class="btn primary" :disabled="!!busy" @click="deposit">
              {{ busy === 'deposit' ? 'Paying…' : `Start round 1 · pay ${usdc(c)}` }}
            </button>
            <span v-if="g.phase === 'active' && me.has_paid" class="chip ok">Paid this round</span>
            <button v-if="me.debt > 0n" class="btn primary" :disabled="!!busy" @click="repay">Repay {{ usdc(me.debt) }}</button>
            <button v-if="g.phase === 'completed' && me.bond + me.escrow > 0n" class="btn primary" :disabled="!!busy" @click="withdraw">
              Withdraw {{ usdc(me.bond + me.escrow) }}
            </button>
            <button v-if="g.phase === 'forming'" class="btn danger" :disabled="!!busy" @click="leave">Leave and refund bond</button>
          </div>
        </template>
        <template v-else-if="g.phase === 'forming' && seatsLeft > 0">
          <p>
            Join as seat <strong>#{{ g.members.length + 1 }}</strong>. You post a <strong>{{ usdc(g.config.bond_amount) }} USDC</strong>
            bond now, then pay {{ usdc(c) }} USDC each round.
          </p>
          <p v-if="balance !== null && balance < g.config.bond_amount + c" class="notice info small">
            You need at least {{ usdc(g.config.bond_amount + c) }} USDC (bond + first round).
            <a v-if="g.config.usdc_token === USDC.sac" :href="USDC.faucet" target="_blank" rel="noopener">Get testnet USDC</a>
          </p>
          <button class="btn primary" :disabled="!!busy" @click="join">{{ busy === 'join' ? 'Joining…' : 'Join guild' }}</button>
        </template>
        <p v-else class="muted">You are not a member of this guild.</p>
      </section>
    </div>

    <!-- Members -->
    <section class="card">
      <div class="spread"><h2>Members in payout order</h2><span class="muted small">Seat order changes only when an unpaid member is moved to the back.</span></div>
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Seat</th><th>Member</th><th>Status</th><th class="num">Rep.</th><th class="num">Bond</th><th class="num">Escrow</th><th class="num">Debt</th><th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(m, i) in g.members" :key="m.address" :class="{ me: m.address === account }">
              <td>#{{ i + 1 }}</td>
              <td class="mono"><a :href="explorer.account(m.address)" target="_blank" rel="noopener">{{ short(m.address) }}</a></td>
              <td><span v-for="s in status(m, i)" :key="s.label" class="chip" :class="s.tone">{{ s.label }}</span></td>
              <td class="num">{{ m.reputation_score }}</td>
              <td class="num">{{ usdc(m.bond) }}</td>
              <td class="num">{{ usdc(m.escrow) }}</td>
              <td class="num" :class="{ bad: m.debt > 0n }">{{ m.debt > 0n ? usdc(m.debt) : '—' }}</td>
              <td>
                <button v-if="g.phase === 'active' && graceOver && !m.has_paid && !m.is_defaulted" class="btn small danger" :disabled="!!busy" @click="flag(m.address)">
                  Flag
                </button>
              </td>
            </tr>
            <tr v-for="n in seatsLeft" :key="`open-${n}`">
              <td>#{{ g.members.length + n }}</td><td colspan="7" class="muted">Open seat</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <div class="grid two">
      <section class="card">
        <h2>How this guild is protected</h2>
        <p class="muted small">
          Bond: {{ usdc(g.config.bond_amount) }} USDC per member. Escrow: {{ g.config.collateral_bps / 100 }}% of a recipient's remaining
          dues, minus the bond, returned round by round. A member who misses before receiving moves to the back and repays from their own pot.
        </p>
        <OutlookTable :contribution="c" :seats="g.config.total_rounds" :bps="g.config.collateral_bps" :highlight="mySeat >= 0 ? mySeat : undefined" />
      </section>
      <section class="card">
        <h2>Rules</h2>
        <dl class="kv">
          <dt>Contribution</dt><dd>{{ usdc(c) }} USDC</dd>
          <dt>Round length</dt><dd>{{ g.config.round_duration_ledgers.toLocaleString() }} ledgers (≈ {{ duration(g.config.round_duration_ledgers) }})</dd>
          <dt>Grace period</dt><dd>≈ {{ duration(g.config.grace_period_ledgers) }}</dd>
          <dt>Fee</dt><dd>0.25% of each pot</dd>
          <dt>Treasury</dt><dd class="mono">{{ short(g.config.treasury) }}</dd>
          <dt>Token</dt><dd class="mono">{{ g.config.usdc_token === USDC.sac ? 'Circle USDC (testnet)' : short(g.config.usdc_token) }}</dd>
          <dt>Ledger</dt><dd>{{ g.ledger.toLocaleString() }}</dd>
        </dl>
        <button v-if="isTreasury" class="btn ghost fees" :disabled="!!busy || g.treasury === 0n" @click="fees">
          Withdraw {{ usdc(g.treasury) }} USDC of fees
        </button>
        <p class="hint">The contract is immutable: these rules cannot be changed by anyone.</p>
      </section>
    </div>
  </template>
</template>

<style scoped>
.head { margin-bottom: 1.25rem; align-items: flex-end; }
.head h1 { margin-top: 0.25rem; }
.grid.stats, .grid.two { margin-bottom: 1rem; }
section.card { margin-bottom: 1rem; }
.bar-text { margin: 0.5rem 0 1rem; }
.actions { margin-top: 1rem; }
.bad { color: var(--danger); font-weight: 600; }
.fees { margin-top: 1rem; }
</style>
