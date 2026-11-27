import { getAddress, getNetworkDetails, isConnected, requestAccess, signTransaction } from '@stellar/freighter-api'
import { ref } from 'vue'
import { NETWORK } from '../config'

/** The connected account, shared by every component. */
export const account = ref<string | null>(null)
export const walletError = ref<string | null>(null)

export async function hasFreighter(): Promise<boolean> {
  const res = await isConnected()
  return !res.error && res.isConnected
}

/** Restore a previous connection without prompting. */
export async function restore() {
  if (!(await hasFreighter())) return
  const res = await getAddress()
  if (!res.error && res.address) account.value = res.address
}

export async function connect() {
  walletError.value = null
  if (!(await hasFreighter())) {
    walletError.value = 'Install the Freighter wallet extension to continue.'
    return
  }
  const res = await requestAccess()
  if (res.error) {
    walletError.value = String(res.error.message ?? res.error)
    return
  }
  const net = await getNetworkDetails()
  if (!net.error && net.networkPassphrase !== NETWORK.passphrase) {
    walletError.value = 'Switch Freighter to Testnet: this app only runs on Stellar testnet.'
  }
  account.value = res.address
}

export function disconnect() {
  account.value = null
}

export async function signXdr(txXdr: string, address: string): Promise<string> {
  const res = await signTransaction(txXdr, { networkPassphrase: NETWORK.passphrase, address })
  if (res.error) throw new Error(String(res.error.message ?? res.error))
  return res.signedTxXdr
}
