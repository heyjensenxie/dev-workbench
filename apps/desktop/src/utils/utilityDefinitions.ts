import type { Component } from 'vue'
import { CalendarClock, Code2, FileDiff, Hash, KeyRound, Link, Radio, RefreshCw, Regex, ShieldCheck, Sparkles } from 'lucide-vue-next'

export type UtilityCategory = 'frequent' | 'data' | 'encoding' | 'security' | 'generators' | 'developer'
export type UtilityId = 'json' | 'jwt' | 'timestamp' | 'regex' | 'text-diff' | 'uuid' | 'base64' | 'url' | 'hash' | 'cron' | 'random'

export interface UtilityDefinition {
  id: UtilityId
  name: string
  description: string
  category: UtilityCategory
  icon: Component
  keywords: string[]
  favorite?: boolean
}

/** One registry keeps navigation, search and future plugin registration on the same contract. */
export const utilityDefinitions: UtilityDefinition[] = [
  { id: 'json', name: 'JSON Workbench', description: 'Format, validate, diff and query JSON', category: 'frequent', icon: Code2, keywords: ['json', 'format', 'minify', 'jsonpath', 'diff'], favorite: true },
  { id: 'jwt', name: 'JWT Inspector', description: 'Decode claims without pretending to verify them', category: 'frequent', icon: KeyRound, keywords: ['jwt', 'token', 'claims', 'decode'], favorite: true },
  { id: 'timestamp', name: 'Time Workbench', description: 'Convert and compare timestamps', category: 'frequent', icon: CalendarClock, keywords: ['timestamp', 'unix', 'iso', 'date', 'time'], favorite: true },
  { id: 'regex', name: 'Regex Playground', description: 'Inspect matches, groups and replacements', category: 'frequent', icon: Regex, keywords: ['regex', 'regexp', 'pattern', 'replace'], favorite: true },
  { id: 'text-diff', name: 'Text Diff', description: 'Compare config, SQL, logs and code locally', category: 'data', icon: FileDiff, keywords: ['diff', 'text', 'compare', 'sql', 'config'], favorite: true },
  { id: 'base64', name: 'Base64', description: 'Encode and decode UTF-8 or Base64URL', category: 'encoding', icon: RefreshCw, keywords: ['base64', 'base64url', 'encode', 'decode'] },
  { id: 'url', name: 'URL Inspector', description: 'Parse, edit and encode URL parameters', category: 'encoding', icon: Link, keywords: ['url', 'query', 'encode', 'decode', 'parser'] },
  { id: 'hash', name: 'Hash & HMAC', description: 'Calculate local digests and keyed hashes', category: 'security', icon: ShieldCheck, keywords: ['hash', 'md5', 'sha', 'hmac'] },
  { id: 'uuid', name: 'UUID Generator', description: 'Generate UUID v4 and v7 in batches', category: 'generators', icon: Hash, keywords: ['uuid', 'v4', 'v7', 'id', 'generate'] },
  { id: 'random', name: 'Random Generator', description: 'Create local strings, passwords and tokens', category: 'generators', icon: Sparkles, keywords: ['random', 'password', 'token', 'hex'] },
  { id: 'cron', name: 'Cron Workbench', description: 'Explain schedules and preview executions', category: 'developer', icon: Radio, keywords: ['cron', 'schedule', 'quartz', 'linux'] },
]

export const utilityCategories: { id: UtilityCategory; label: string; ids: UtilityId[] }[] = [
  { id: 'frequent', label: 'Frequent', ids: ['json', 'jwt', 'timestamp', 'regex'] },
  { id: 'data', label: 'Data', ids: ['json', 'text-diff'] },
  { id: 'encoding', label: 'Encoding', ids: ['base64', 'url'] },
  { id: 'security', label: 'Security', ids: ['jwt', 'hash'] },
  { id: 'generators', label: 'Generators', ids: ['uuid', 'random'] },
  { id: 'developer', label: 'Developer', ids: ['regex', 'cron', 'timestamp'] },
]

export function getUtilityDefinition(id: UtilityId): UtilityDefinition {
  return utilityDefinitions.find((definition) => definition.id === id) ?? utilityDefinitions[0]!
}
