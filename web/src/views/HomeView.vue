<script setup lang="ts">
import { StrKey } from '@stellar/stellar-sdk'
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { recentGuilds } from '../lib/guild'
import { short } from '../lib/format'
import { DEMO_GUILDS } from '../demo'

const router = useRouter()
const input = ref('')
const recent = recentGuilds()
const valid = computed(() => StrKey.isValidContract(input.value.trim()))

function open() {
  if (valid.value) router.push(`/guild/${input.value.trim()}`)
}
</script>

<template>
  <section class="hero">
    <p class="eyebrow">Esusu · Tanda · Chama · Partner</p>
    <h1>A savings circle where nobody holds the money, and nobody can walk away with it.</h1>
    <p class="lead muted">
      Members pay the same USDC amount every round and the whole pot goes to one member in turn. A Soroban contract keeps
      the money and the order. Bonds and escrow mean an early recipient who stops paying is covered, not just named.
    </p>
    <div class="row">
      <RouterLink to="/create" class="btn primary">Start a guild</RouterLink>
      <a href="#open" class="btn ghost">Open an existing guild</a>
    </div>
  </section>

  <section class="grid three steps">
    <div class="card">
      <span class="step">1</span>
      <h3>Join and post a bond</h3>
      <p class="muted small">
        Each member locks one contribution when taking a seat. Seat order is payout order. You can leave and get the bond
        back until the first contribution.
      </p>
    </div>
    <div class="card">
      <span class="step">2</span>
      <h3>Pay each round, receive once</h3>
      <p class="muted small">
        When the round ends, anyone can trigger the payout. The pot goes to that round's seat, minus a 0.25% fee.
      </p>
    </div>
    <div class="card">
      <span class="step">3</span>
      <h3>Defaults are paid for</h3>
      <p class="muted small">
        A missed round is paid from the defaulter's escrow and bond. Someone who has not received yet moves to the back,
        and their own pot repays whoever was short-changed.
      </p>
    </div>
  </section>

  <section id="open" class="grid two">
    <div class="card">
      <h2>Open a guild</h2>
      <form class="stack" @submit.prevent="open">
        <label>
          Guild contract ID
          <input v-model="input" placeholder="C…" spellcheck="false" autocomplete="off" />
        </label>
        <button class="btn primary" :disabled="!valid">Open</button>
      </form>
      <div v-if="recent.length" class="recent">
        <h3>Recently opened</h3>
        <RouterLink v-for="g in recent" :key="g" :to="`/guild/${g}`" class="mono">{{ short(g) }}</RouterLink>
      </div>
    </div>
    <div class="card">
      <h2>Testnet evidence</h2>
      <p class="muted small">
        Guilds run end to end on testnet by <code>scripts/testnet-demo.sh</code>. Open one to see its final state.
      </p>
      <ul class="demo-list">
        <li v-for="d in DEMO_GUILDS" :key="d.id">
          <RouterLink :to="`/guild/${d.id}`">{{ d.title }}</RouterLink>
          <span class="muted small">{{ d.summary }}</span>
        </li>
      </ul>
    </div>
  </section>
</template>

<style scoped>
.hero { padding: 2rem 0 2.5rem; max-width: 760px; }
.eyebrow { color: var(--gold); font-weight: 600; letter-spacing: 0.04em; font-size: 0.85rem; }
.lead { font-size: 1.1rem; max-width: 640px; }
.steps { margin-bottom: 2rem; }
.step {
  display: inline-grid; place-items: center; width: 1.8rem; height: 1.8rem; border-radius: 50%;
  background: var(--accent-soft); color: var(--accent); font-weight: 700; margin-bottom: 0.6rem;
}
.recent { margin-top: 1.25rem; display: flex; flex-wrap: wrap; gap: 0.5rem 1rem; align-items: baseline; }
.recent h3 { width: 100%; font-size: 0.85rem; color: var(--muted); margin: 0; }
.demo-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.75rem; }
.demo-list li { display: flex; flex-direction: column; }
</style>
