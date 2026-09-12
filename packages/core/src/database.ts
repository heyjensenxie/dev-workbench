import type { DatabaseType, QueryResult } from '@dev-workbench/shared'

export type SqlSafetyWarningKind = 'drop' | 'truncate' | 'update-without-where' | 'delete-without-where' | 'alter'

/**
 * A locale-neutral safety finding. Copy lives in the presentation layer so the
 * same analysis can be rendered in any interface language.
 */
export interface SqlSafetyWarning {
  kind: SqlSafetyWarningKind
}

/** Splits a script without breaking semicolons inside quoted SQL strings. */
export function splitSqlStatements(sql: string): string[] {
  const statements: string[] = []
  let current = ''
  let quote: 'single' | 'double' | 'backtick' | undefined
  let escaped = false
  for (const character of sql) {
    if (escaped) {
      current += character
      escaped = false
      continue
    }
    if (character === '\\' && quote) {
      current += character
      escaped = true
      continue
    }
    if (!quote && (character === "'" || character === '"' || character === '`')) {
      quote = character === "'" ? 'single' : character === '"' ? 'double' : 'backtick'
    } else if (quote && ((quote === 'single' && character === "'") || (quote === 'double' && character === '"') || (quote === 'backtick' && character === '`'))) {
      quote = undefined
    }
    if (character === ';' && !quote) {
      if (current.trim()) statements.push(current.trim())
      current = ''
    } else {
      current += character
    }
  }
  if (current.trim()) statements.push(current.trim())
  return statements
}

export function analyzeSqlSafety(sql: string): SqlSafetyWarning[] {
  const normalized = sql.trim().replace(/\s+/g, ' ').toLocaleLowerCase()
  const warnings: SqlSafetyWarning[] = []
  if (/^drop\s+(database|table)\b/.test(normalized)) warnings.push({ kind: 'drop' })
  if (/^truncate\s+table\b/.test(normalized)) warnings.push({ kind: 'truncate' })
  if (/^alter\s+table\b/.test(normalized)) warnings.push({ kind: 'alter' })
  if (/^update\b/.test(normalized) && !/\bwhere\b/.test(normalized)) warnings.push({ kind: 'update-without-where' })
  if (/^delete\s+from\b/.test(normalized) && !/\bwhere\b/.test(normalized)) warnings.push({ kind: 'delete-without-where' })
  return warnings
}

export function isReadQuery(sql: string): boolean {
  return /^(select|with|show|describe|desc|pragma|explain)\b/i.test(sql.trim())
}

export function applyResultLimit(sql: string, limit: number): string {
  const normalizedLimit = Math.max(1, Math.round(limit))
  const statement = sql.trim()
  if (!/^select\b/i.test(statement) || /\blimit\s+\d+/i.test(statement)) return statement
  return `${statement.replace(/;\s*$/, '')}\nLIMIT ${normalizedLimit};`
}

export function inferColumnType(value: unknown): string {
  if (value === null || value === undefined) return 'NULL'
  if (typeof value === 'boolean') return 'BOOLEAN'
  if (typeof value === 'number') return 'NUMBER'
  if (typeof value === 'object') return 'JSON'
  if (typeof value === 'string' && /^\d{4}-\d{2}-\d{2}/.test(value)) return 'DATE'
  return 'TEXT'
}

/** Keeps the result grid contract stable for adapters that omit column metadata. */
export function withInferredColumns(result: QueryResult): QueryResult {
  if (result.columns.length || !result.rows.length) return result
  const firstRow = result.rows[0] ?? {}
  return { ...result, columns: Object.keys(firstRow).map((name) => ({ name, type: inferColumnType(firstRow[name]) })) }
}

export function databaseLabel(type: DatabaseType): string {
  return type === 'mysql' ? 'MySQL' : type === 'sqlite' ? 'SQLite' : 'PostgreSQL'
}
