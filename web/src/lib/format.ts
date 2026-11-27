import { NETWORK, SECONDS_PER_LEDGER, USDC } from '../config'

const UNIT = 10n ** BigInt(USDC.decimals)

/** Stroops to a USDC string with at most 2–7 decimals, no trailing zeros past 2. */
export function usdc(stroops: bigint | number): string {
  const v = BigInt(stroops)
  const neg = v < 0n
  const abs = neg ? -v : v
  const whole = abs / UNIT
  let frac = (abs % UNIT).toString().padStart(USDC.decimals, '0').replace(/0+$/, '')
  if (frac.length < 2) frac = frac.padEnd(2, '0')
  return `${neg ? '−' : ''}${whole.toLocaleString('en-US')}.${frac}`
}

/** A decimal USDC string to stroops. Returns null for invalid input. */
export function toStroops(value: string): bigint | null {
  const m = value.trim().match(/^(\d+)(?:\.(\d{0,7}))?$/)
  if (!m) return null
  return BigInt(m[1]) * UNIT + BigInt((m[2] ?? '').padEnd(USDC.decimals, '0'))
}

/** Ledger count to a rough human duration. */
export function duration(ledgers: number): string {
  const s = Math.max(0, ledgers) * SECONDS_PER_LEDGER
  if (s < 90) return `${s}s`
  const m = Math.round(s / 60)
  if (m < 90) return `${m} min`
  const h = Math.round(m / 6) / 10
  if (h < 48) return `${h} h`
  return `${Math.round(h / 24)} days`
}

export function short(address: string): string {
  return `${address.slice(0, 4)}…${address.slice(-4)}`
}

export const explorer = {
  account: (a: string) => `${NETWORK.explorer}/${a.startsWith('C') ? 'contract' : 'account'}/${a}`,
  tx: (h: string) => `${NETWORK.explorer}/tx/${h}`,
}
