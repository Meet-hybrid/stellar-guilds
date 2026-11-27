<script setup lang="ts">
import { computed } from 'vue'
import { outlook } from '../lib/guild'
import { usdc } from '../lib/format'

const props = defineProps<{ contribution: bigint; seats: number; bps: number; compact?: boolean; highlight?: number }>()

const rows = computed(() => outlook(props.contribution, Math.min(Math.max(props.seats, 2), 50), props.bps))
const worst = computed(() => rows.value[0]?.unbacked ?? 0n)
</script>

<template>
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th>Seat</th>
          <th class="num">Paid out</th>
          <th class="num">Held</th>
          <th class="num">Unbacked</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in compact ? rows.slice(0, 6) : rows" :key="r.round" :class="{ me: highlight === r.round }">
          <td>#{{ r.round + 1 }}</td>
          <td class="num">{{ usdc(r.paidNow) }}</td>
          <td class="num">{{ usdc(r.held) }}</td>
          <td class="num" :class="{ bad: r.unbacked > 0n }">{{ usdc(r.unbacked) }}</td>
        </tr>
        <tr v-if="compact && rows.length > 6">
          <td colspan="4" class="muted small">… {{ rows.length - 6 }} more seats</td>
        </tr>
      </tbody>
    </table>
  </div>
  <p class="small summary" :class="worst > 0n ? 'warnline' : 'okline'">
    <template v-if="worst === 0n">Every seat's remaining dues are fully backed by bond and escrow.</template>
    <template v-else>
      Worst case: if seat #1 takes the pot and never pays again, {{ usdc(worst) }} USDC of later pots is backed only by
      recorded debt.
    </template>
  </p>
</template>

<style scoped>
td.bad { color: var(--danger); font-weight: 600; }
.summary { margin-top: 0.75rem; }
.warnline { color: var(--gold); }
.okline { color: var(--ok); }
</style>
