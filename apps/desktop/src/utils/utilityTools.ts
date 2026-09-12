export type TimestampUnit = 'seconds' | 'milliseconds' | 'date'

export interface TimestampResult {
  unit: TimestampUnit
  milliseconds: number
  seconds: number
  iso: string
  local: string
}

export interface JwtResult {
  header: unknown
  payload: unknown
}

export interface JwtInspectorResult extends JwtResult {
  signature: string
  algorithm: string
  claims: Record<string, unknown>
}

export interface JsonDiagnostic {
  message: string
  line: number
  column: number
}

export interface DiffLine {
  type: 'added' | 'removed' | 'unchanged'
  value: string
  line: number
}

export interface JsonDiffEntry {
  type: 'added' | 'removed' | 'changed'
  path: string
  before?: unknown
  after?: unknown
}

export interface UrlParts {
  protocol: string
  host: string
  port: string
  path: string
  query: string
  fragment: string
  params: { key: string; value: string }[]
}

export interface RegexMatch {
  value: string
  index: number
  groups: string[]
}

export interface RegexTestResult {
  matches: RegexMatch[]
  truncated: boolean
}

const MAX_REGEX_MATCHES = 1000

/** Parses JSON once and keeps formatting decisions outside the view layer. */
export function formatJson(value: string, compact = false): string {
  return JSON.stringify(JSON.parse(value) as unknown, null, compact ? 0 : 2)
}

/** Returns a user-facing location for malformed JSON without leaking parser internals. */
export function diagnoseJson(value: string): JsonDiagnostic | undefined {
  try {
    JSON.parse(value)
    return undefined
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Invalid JSON'
    const position = message.match(/position\s+(\d+)/i)?.[1]
    const offset = position ? Number(position) : Math.max(0, value.length - 1)
    const before = value.slice(0, offset)
    return { message: message.replace(/\s+at position\s+\d+/i, ''), line: before.split('\n').length, column: offset - before.lastIndexOf('\n') }
  }
}

/** Sorts object keys recursively while leaving arrays in their original order. */
export function sortJsonKeys(value: string): string {
  const sort = (input: unknown): unknown => {
    if (Array.isArray(input)) return input.map(sort)
    if (input && typeof input === 'object') return Object.fromEntries(Object.entries(input).sort(([a], [b]) => a.localeCompare(b)).map(([key, child]) => [key, sort(child)]))
    return input
  }
  return JSON.stringify(sort(JSON.parse(value) as unknown), null, 2)
}

/** Compares parsed values by path so JSON Diff can explain changes beyond text offsets. */
export function diffJson(original: string, changed: string): JsonDiffEntry[] {
  const left = JSON.parse(original) as unknown
  const right = JSON.parse(changed) as unknown
  const rows: JsonDiffEntry[] = []
  const walk = (before: unknown, after: unknown, path: string): void => {
    if (before === undefined) { rows.push({ type: 'added', path, after }); return }
    if (after === undefined) { rows.push({ type: 'removed', path, before }); return }
    if (Object.is(before, after)) return
    if (Array.isArray(before) && Array.isArray(after)) { const size = Math.max(before.length, after.length); for (let i = 0; i < size; i++) walk(before[i], after[i], `${path}[${i}]`); return }
    if (before && after && typeof before === 'object' && typeof after === 'object') { const keys = new Set([...Object.keys(before), ...Object.keys(after)]); for (const key of keys) walk((before as Record<string, unknown>)[key], (after as Record<string, unknown>)[key], `${path}.${key}`); return }
    rows.push({ type: 'changed', path, before, after })
  }
  walk(left, right, '$')
  return rows
}

/** Escapes JSON into a string literal and reverses that operation safely. */
export function escapeJson(value: string): string { return JSON.stringify(value) }
export function unescapeJson(value: string): string { return JSON.parse(value) as string }

/** Encodes UTF-8 text instead of treating JavaScript characters as bytes. */
export function encodeBase64(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

/** Decodes UTF-8 Base64 and rejects malformed or non-UTF-8 input. */
export function decodeBase64(value: string): string {
  const binary = atob(value.replace(/\s/g, ''))
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0))
  return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
}

/** Encodes a value for use in a URL component while preserving Unicode text. */
export function encodeUrlComponent(value: string): string {
  return encodeURIComponent(value)
}

/** Decodes a URL component and surfaces malformed percent escapes to the UI. */
export function decodeUrlComponent(value: string): string {
  return decodeURIComponent(value)
}

/** Decodes only the inspectable JWT parts; it deliberately does not verify signatures. */
export function decodeJwt(token: string): JwtResult {
  const parts = token.trim().split('.')
  if (parts.length !== 3 || parts.some((part) => !part)) throw new Error('invalid JWT')
  return {
    header: parseJson(decodeBase64Url(parts[0]!)),
    payload: parseJson(decodeBase64Url(parts[1]!)),
  }
}

/** Decodes inspectable JWT metadata. Signature remains opaque until explicit verification exists. */
export function inspectJwt(token: string): JwtInspectorResult {
  const parts = token.trim().split('.')
  if (parts.length !== 3 || parts.some((part) => !part)) throw new Error('invalid JWT')
  const header = parseJson(decodeBase64Url(parts[0]!))
  const payload = parseJson(decodeBase64Url(parts[1]!))
  return { header, payload, signature: parts[2]!, algorithm: typeof header === 'object' && header !== null && 'alg' in header ? String(header.alg) : 'unknown', claims: typeof payload === 'object' && payload !== null ? payload as Record<string, unknown> : {} }
}

export function claimDate(value: unknown): Date | undefined {
  if (typeof value !== 'number' || !Number.isFinite(value)) return undefined
  const date = new Date(value * 1000)
  return Number.isNaN(date.getTime()) ? undefined : date
}

/** Converts Unix seconds/milliseconds or a date string into a comparable result. */
export function formatTimestamp(input: string): TimestampResult {
  const value = input.trim()
  if (!value) throw new Error('timestamp is empty')

  const numeric = /^[-+]?\d+(?:\.\d+)?$/.test(value) ? Number(value) : Number.NaN
  const unit: TimestampUnit = Number.isFinite(numeric)
    ? Math.abs(numeric) < 100_000_000_000 ? 'seconds' : 'milliseconds'
    : 'date'
  const milliseconds = unit === 'seconds' ? numeric * 1000 : unit === 'milliseconds' ? numeric : Date.parse(value)
  const date = new Date(milliseconds)
  if (!Number.isFinite(milliseconds) || Number.isNaN(date.getTime())) throw new Error('invalid timestamp')
  return {
    unit,
    milliseconds,
    seconds: milliseconds / 1000,
    iso: date.toISOString(),
    local: date.toString(),
  }
}

/** Runs a regular expression globally so callers can inspect every match. */
export function testRegex(pattern: string, flags: string, input: string): RegexTestResult {
  const globalFlags = flags.includes('g') ? flags : `${flags}g`
  const expression = new RegExp(pattern, globalFlags)
  const matches: RegexMatch[] = []
  let match: RegExpExecArray | null
  while ((match = expression.exec(input)) !== null) {
    const groups = match.slice(1)
    const namedGroups = match.groups ? Object.entries(match.groups).map(([name, value]) => `${name}=${value}`) : []
    matches.push({ value: match[0], index: match.index, groups: [...groups, ...namedGroups] })
    if (matches.length >= MAX_REGEX_MATCHES) return { matches, truncated: true }
    // RegExp#exec does not advance after an empty match in every engine.
    if (match[0] === '') expression.lastIndex += 1
  }
  return { matches, truncated: false }
}

export type HashAlgorithm = 'MD5' | 'SHA-1' | 'SHA-256' | 'SHA-384' | 'SHA-512'

/** Hashes local text with the browser's Web Crypto implementation. */
export async function hashText(value: string, algorithm: HashAlgorithm): Promise<string> {
  if (!globalThis.crypto?.subtle) throw new Error('Web Crypto is unavailable')
  if (algorithm === 'MD5') return md5Hex(new TextEncoder().encode(value))
  const buffer = await globalThis.crypto.subtle.digest(algorithm, new TextEncoder().encode(value))
  return Array.from(new Uint8Array(buffer), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

/** Calculates supported Web Crypto digests together, avoiding repeated input setup in the view. */
export async function hashAll(value: string): Promise<Record<Exclude<HashAlgorithm, 'MD5'> | 'MD5', string>> {
  const algorithms: HashAlgorithm[] = ['MD5', 'SHA-1', 'SHA-256', 'SHA-384', 'SHA-512']
  const entries = await Promise.all(algorithms.map(async (algorithm) => [algorithm, await hashText(value, algorithm)] as const))
  return Object.fromEntries(entries) as Record<HashAlgorithm, string>
}

/** Calculates an HMAC in memory; callers intentionally own the secret lifecycle. */
export async function hmacText(value: string, secret: string, algorithm: 'HMAC-SHA256' | 'HMAC-SHA512'): Promise<string> {
  if (!globalThis.crypto?.subtle) throw new Error('Web Crypto is unavailable')
  const key = await globalThis.crypto.subtle.importKey('raw', new TextEncoder().encode(secret), { name: 'HMAC', hash: algorithm === 'HMAC-SHA256' ? 'SHA-256' : 'SHA-512' }, false, ['sign'])
  const buffer = await globalThis.crypto.subtle.sign('HMAC', key, new TextEncoder().encode(value))
  return Array.from(new Uint8Array(buffer), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

export function parseUrl(value: string): UrlParts {
  const url = new URL(value)
  return { protocol: url.protocol.replace(/:$/, ''), host: url.hostname, port: url.port, path: url.pathname, query: url.search.replace(/^\?/, ''), fragment: url.hash.replace(/^#/, ''), params: Array.from(url.searchParams.entries()).map(([key, entryValue]) => ({ key, value: entryValue })) }
}

/** Produces a stable line-level diff for pasted text and config files. */
export function diffText(original: string, changed: string, options: { ignoreWhitespace?: boolean; ignoreCase?: boolean; ignoreEmptyLines?: boolean } = {}): DiffLine[] {
  const normalize = (line: string) => options.ignoreCase ? line.toLowerCase() : line
  const prepare = (value: string) => value.split('\n').filter((line) => !options.ignoreEmptyLines || line.trim() !== '')
  const left = prepare(original)
  const right = prepare(changed)
  const same = (a: string, b: string) => normalize(options.ignoreWhitespace ? a.replace(/\s+/g, '') : a) === normalize(options.ignoreWhitespace ? b.replace(/\s+/g, '') : b)
  const rows: DiffLine[] = []
  let i = 0; let j = 0
  while (i < left.length || j < right.length) {
    if (i < left.length && j < right.length && same(left[i]!, right[j]!)) { rows.push({ type: 'unchanged', value: right[j]!, line: j + 1 }); i++; j++; continue }
    if (i + 1 < left.length && j < right.length && same(left[i + 1]!, right[j]!)) { rows.push({ type: 'removed', value: left[i]!, line: i + 1 }); i++; continue }
    if (j + 1 < right.length && i < left.length && same(left[i]!, right[j + 1]!)) { rows.push({ type: 'added', value: right[j]!, line: j + 1 }); j++; continue }
    if (i < left.length) { rows.push({ type: 'removed', value: left[i]!, line: i + 1 }); i++ }
    if (j < right.length) { rows.push({ type: 'added', value: right[j]!, line: j + 1 }); j++ }
  }
  return rows
}

function decodeBase64Url(value: string): string {
  const normalized = value.replace(/-/g, '+').replace(/_/g, '/')
  return decodeBase64(normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '='))
}

function parseJson(value: string): unknown {
  return JSON.parse(value) as unknown
}

function md5Hex(bytes: Uint8Array): string {
  // MD5 is included for checksums only. It never claims cryptographic security.
  const words = Array.from(bytes); const bitLength = words.length * 8
  words.push(0x80); while (words.length % 64 !== 56) words.push(0)
  for (let i = 0; i < 8; i++) words.push((bitLength / 2 ** (8 * i)) & 0xff)
  let a = 0x67452301; let b = 0xefcdab89; let c = 0x98badcfe; let d = 0x10325476
  const shifts = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21]
  for (let offset = 0; offset < words.length; offset += 64) {
    const block = Array.from({ length: 16 }, (_, i) => words[offset + i * 4]! | words[offset + i * 4 + 1]! << 8 | words[offset + i * 4 + 2]! << 16 | words[offset + i * 4 + 3]! << 24)
    let aa = a; let bb = b; let cc = c; let dd = d
    for (let i = 0; i < 64; i++) {
      let f: number; let g: number
      if (i < 16) { f = (bb & cc) | (~bb & dd); g = i } else if (i < 32) { f = (dd & bb) | (~dd & cc); g = (5 * i + 1) % 16 } else if (i < 48) { f = bb ^ cc ^ dd; g = (3 * i + 5) % 16 } else { f = cc ^ (bb | ~dd); g = (7 * i) % 16 }
      const k = Math.floor(Math.abs(Math.sin(i + 1)) * 2 ** 32)
      const rotation = shifts[(Math.floor(i / 16) * 4 + i % 4)]!
      const next = (aa + f + k + block[g]!) >>> 0
      aa = dd; dd = cc; cc = bb; bb = (bb + ((next << rotation) | (next >>> (32 - rotation)))) >>> 0
    }
    a = (a + aa) >>> 0; b = (b + bb) >>> 0; c = (c + cc) >>> 0; d = (d + dd) >>> 0
  }
  return [a, b, c, d].map((word) => [0, 8, 16, 24].map((shift) => ((word >>> shift) & 0xff).toString(16).padStart(2, '0')).join('')).join('')
}
