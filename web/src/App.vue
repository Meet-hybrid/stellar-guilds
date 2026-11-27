<script setup lang="ts">
import { onMounted } from 'vue'
import { account, connect, disconnect, restore, walletError } from './lib/wallet'
import { short } from './lib/format'

onMounted(restore)
</script>

<template>
  <header class="topbar">
    <RouterLink to="/" class="brand">
      <span class="brand-mark" aria-hidden="true">◎</span>
      Stellar Guilds
    </RouterLink>
    <nav class="topnav">
      <RouterLink to="/create">Create a guild</RouterLink>
      <span class="net-pill">Testnet</span>
      <button v-if="account" class="btn ghost" :title="account" @click="disconnect">
        {{ short(account) }}
      </button>
      <button v-else class="btn primary" @click="connect">Connect wallet</button>
    </nav>
  </header>
  <p v-if="walletError" class="banner warn" role="alert">{{ walletError }}</p>

  <main class="page">
    <RouterView />
  </main>

  <footer class="footer">
    <span>Open source · MIT</span>
    <a href="https://github.com/Meet-hybrid/stellar-guilds" target="_blank" rel="noopener">GitHub</a>
    <span>Testnet only. Test tokens have no value.</span>
  </footer>
</template>
