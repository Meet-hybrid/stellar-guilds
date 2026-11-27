import { Networks } from '@stellar/stellar-sdk'

/** Stellar testnet. The contract is not deployed to mainnet. */
export const NETWORK = {
  name: 'Testnet',
  passphrase: Networks.TESTNET,
  rpcUrl: import.meta.env.VITE_RPC_URL ?? 'https://soroban-testnet.stellar.org',
  explorer: 'https://stellar.expert/explorer/testnet',
}

/**
 * SHA-256 of the guild-vault wasm already uploaded to testnet. "Create a
 * guild" deploys a fresh instance of exactly this code.
 */
export const GUILD_WASM_HASH =
  import.meta.env.VITE_GUILD_WASM_HASH ??
  '9b15e61351a4269a9f6a43940c7da6885385dd18f52738f43bd05ba5dde01777'

/** Circle's testnet USDC, wrapped as a Stellar Asset Contract. */
export const USDC = {
  code: 'USDC',
  issuer: 'GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5',
  sac: 'CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA',
  faucet: 'https://faucet.circle.com/',
  decimals: 7,
}

/** Average testnet close time, used only to turn ledgers into human durations. */
export const SECONDS_PER_LEDGER = 5

/** Names for the contract's typed errors (`types.rs`). */
export const CONTRACT_ERRORS: Record<number, string> = {
  1: 'This guild is already initialized.',
  2: 'This contract has not been initialized as a guild.',
  3: 'You are already a member of this guild.',
  4: 'This address is not a member of the guild.',
  5: 'Every seat is taken.',
  6: 'You have already contributed this round.',
  7: 'The round deadline has not been reached yet.',
  8: 'Some members have not paid and the grace period is still running.',
  9: 'The grace period has not expired yet.',
  10: 'This member is already flagged as defaulted.',
  11: 'There are no fees to withdraw.',
  12: 'One of the guild parameters is out of range.',
  13: 'Arithmetic overflow.',
  14: 'Every round has been settled.',
  15: 'The guild has not started yet.',
  16: 'The contribution window for this round is closed.',
  17: 'The guild cannot start until every seat is filled.',
  18: 'This member has already paid this round.',
  19: 'The guild has started, so membership is frozen.',
  20: 'The cycle is still running.',
  21: 'There is no collateral left to withdraw.',
  22: 'There is no debt to repay.',
}
