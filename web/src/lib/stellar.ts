import {
  Account,
  Address,
  Keypair,
  BASE_FEE,
  Contract,
  Operation,
  TransactionBuilder,
  nativeToScVal,
  rpc,
  scValToNative,
  xdr,
} from '@stellar/stellar-sdk'
import { Buffer } from 'buffer'
import { CONTRACT_ERRORS, NETWORK } from '../config'
import { signXdr } from './wallet'

export const server = new rpc.Server(NETWORK.rpcUrl)

/** Simulation does not need a funded source account, so reads use a throwaway one. */
const simulationSource = new Account(Keypair.random().publicKey(), '0')

export class ContractError extends Error {
  readonly code?: number
  constructor(message: string, code?: number) {
    super(message)
    this.code = code
  }
}

/** Turn a simulation or submission failure into something a person can read. */
function explain(raw: string): ContractError {
  const match = raw.match(/Error\(Contract, #(\d+)\)/)
  if (match) {
    const code = Number(match[1])
    return new ContractError(CONTRACT_ERRORS[code] ?? `Contract error #${code}`, code)
  }
  if (/trustline entry is missing|TrustlineMissing/i.test(raw)) {
    return new ContractError('Your account has no USDC trustline. Add one in your wallet, then fund it from the faucet.')
  }
  if (/balance is not sufficient|insufficient/i.test(raw)) {
    return new ContractError('Not enough USDC in your account for this action.')
  }
  const firstLine = raw.split('\n').find((l) => l.trim()) ?? raw
  return new ContractError(firstLine.slice(0, 240))
}

/** Build, simulate and return the native result of a read-only call. */
export async function read<T>(contractId: string, method: string, ...args: xdr.ScVal[]): Promise<T> {
  const tx = new TransactionBuilder(simulationSource, { fee: BASE_FEE, networkPassphrase: NETWORK.passphrase })
    .addOperation(new Contract(contractId).call(method, ...args))
    .setTimeout(30)
    .build()
  const sim = await server.simulateTransaction(tx)
  if (rpc.Api.isSimulationError(sim)) throw explain(sim.error)
  const retval = sim.result?.retval
  return (retval ? scValToNative(retval) : undefined) as T
}

export interface SentTx {
  hash: string
  returnValue?: xdr.ScVal
}

/** Simulate, sign with the connected wallet, submit and wait for the result. */
export async function send(source: string, operation: xdr.Operation): Promise<SentTx> {
  const account = await server.getAccount(source).catch(() => {
    throw new ContractError('This account does not exist on testnet yet. Fund it with Friendbot first.')
  })
  const built = new TransactionBuilder(account, { fee: BASE_FEE, networkPassphrase: NETWORK.passphrase })
    .addOperation(operation)
    .setTimeout(120)
    .build()

  const sim = await server.simulateTransaction(built)
  if (rpc.Api.isSimulationError(sim)) throw explain(sim.error)
  const prepared = rpc.assembleTransaction(built, sim).build()

  const signed = await signXdr(prepared.toXDR(), source)
  const tx = TransactionBuilder.fromXDR(signed, NETWORK.passphrase)
  const sent = await server.sendTransaction(tx)
  if (sent.status === 'ERROR') {
    throw explain(sent.errorResult?.toXDR('base64') ?? 'The network rejected the transaction.')
  }

  for (let i = 0; i < 40; i++) {
    const res = await server.getTransaction(sent.hash)
    if (res.status === rpc.Api.GetTransactionStatus.SUCCESS) {
      return { hash: sent.hash, returnValue: res.returnValue }
    }
    if (res.status === rpc.Api.GetTransactionStatus.FAILED) {
      throw new ContractError(`Transaction failed on-chain (${sent.hash.slice(0, 8)}…).`)
    }
    await new Promise((r) => setTimeout(r, 1500))
  }
  throw new ContractError(`Timed out waiting for ${sent.hash.slice(0, 8)}…; check the explorer.`)
}

export function invoke(source: string, contractId: string, method: string, ...args: xdr.ScVal[]) {
  return send(source, new Contract(contractId).call(method, ...args))
}

/** Deploy a fresh contract instance from wasm already uploaded to the network. */
export async function deployFromHash(source: string, wasmHash: string): Promise<string> {
  const salt = Buffer.from(crypto.getRandomValues(new Uint8Array(32)))
  const op = Operation.createCustomContract({
    address: Address.fromString(source),
    wasmHash: Buffer.from(wasmHash, 'hex'),
    salt,
  })
  const { returnValue } = await send(source, op)
  if (!returnValue) throw new ContractError('Deployment returned no contract address.')
  return Address.fromScVal(returnValue).toString()
}

export async function latestLedger(): Promise<number> {
  return (await server.getLatestLedger()).sequence
}

export async function tokenBalance(tokenId: string, who: string): Promise<bigint> {
  try {
    return BigInt(await read<bigint>(tokenId, 'balance', addr(who)))
  } catch {
    return 0n
  }
}

// --- ScVal helpers ---------------------------------------------------------

export const addr = (a: string) => new Address(a).toScVal()
export const u32 = (n: number) => nativeToScVal(n, { type: 'u32' })
export const i128 = (n: bigint) => nativeToScVal(n, { type: 'i128' })

/** A `#[contracttype]` struct is an ScMap with symbol keys in sorted order. */
export function struct(fields: Record<string, xdr.ScVal>): xdr.ScVal {
  return xdr.ScVal.scvMap(
    Object.keys(fields)
      .sort()
      .map((k) => new xdr.ScMapEntry({ key: xdr.ScVal.scvSymbol(k), val: fields[k] })),
  )
}
