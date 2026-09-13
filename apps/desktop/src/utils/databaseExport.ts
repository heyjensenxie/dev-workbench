import type { QueryResult } from '@dev-workbench/shared'

export type DatabaseExportFormat = 'csv' | 'json'

function exportValue(value: unknown): string {
  if (value === null || value === undefined) return ''
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

function escapeCsv(value: unknown): string {
  const text = exportValue(value)
  return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text
}

/** Serializes the visible query columns in their displayed order. */
export function queryResultToCsv(result: QueryResult): string {
  const header = result.columns.map((column) => escapeCsv(column.name)).join(',')
  const rows = result.rows.map((row) => result.columns.map((column) => escapeCsv(row[column.name])).join(','))
  return [header, ...rows].join('\r\n')
}

/** Exports rows as records, preserving null values and the displayed column order. */
export function queryResultToJson(result: QueryResult): string {
  const rows = result.rows.map((row) => Object.fromEntries(result.columns.map((column) => [column.name, row[column.name] ?? null])))
  return JSON.stringify(rows, null, 2)
}
